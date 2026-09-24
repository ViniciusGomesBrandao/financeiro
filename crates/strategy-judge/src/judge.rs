use std::collections::HashMap;

use chrono::{DateTime, Utc};
use domain::{InstrumentId, Position, PositionStatus, StrategyId};
use features::FeatureSnapshot;
use rust_decimal::Decimal;

use crate::affinity::{regime_affinity, strategy_kind_from_id};
use crate::decision::{JudgeDecision, JudgeMetrics, JudgeReason, JudgeState};
use crate::regime::{
    classify_regime, MarketRegime, RegimeAssessment, RegimeEvidence, RegimeThresholds,
};
use crate::selection::{CandidateFit, SelectionOutcome, SelectionReason};
use crate::thresholds::JudgeThresholds;

/// O que o Judge lembra sobre um par `(InstrumentId, StrategyId)` entre
/// chamadas — o mecanismo anti-ruído depende inteiramente disto: sem
/// memória, cada avaliação seria uma função pura da amostra mais recente,
/// e uma única má avaliação já trocaria a recomendação.
#[derive(Debug, Clone)]
struct Memory {
    /// O último estado efetivamente adotado (não necessariamente o
    /// "cru", proposto pela avaliação mais recente).
    state: JudgeState,
    /// Uma transição ainda não confirmada: o estado para o qual as
    /// avaliações recentes vêm apontando, e quantas vezes consecutivas
    /// isso já aconteceu.
    pending: Option<(JudgeState, u32)>,
}

/// Histerese da *seleção* ativa por contexto (robô), usada quando a
/// escolha é guiada por regime — evita churn a cada candle.
#[derive(Debug, Clone)]
struct SelectionMemory {
    selected: Option<StrategyId>,
    pending: Option<(Option<StrategyId>, u32)>,
}

/// Classifica `metrics` contra `thresholds`, sem nenhuma memória de
/// avaliações anteriores — a "opinião crua" desta amostra sozinha.
/// `StrategyJudge::evaluate` aplica a histerese por cima disto.
fn classify(metrics: &JudgeMetrics, thresholds: &JudgeThresholds) -> (JudgeState, JudgeReason) {
    if metrics.trades < thresholds.min_sample_size {
        return (
            JudgeState::Disabled,
            JudgeReason::InsufficientSample {
                trades: metrics.trades,
                required: thresholds.min_sample_size,
            },
        );
    }

    let mut failures = Vec::new();

    if metrics.win_rate < thresholds.min_win_rate {
        failures.push(JudgeReason::BelowConsistencyThreshold {
            win_rate: metrics.win_rate,
            min: thresholds.min_win_rate,
        });
    }
    // `None` (nenhum trade perdedor na amostra) nunca reprova — é o
    // melhor caso possível, não uma ausência de dado.
    if let Some(profit_factor) = metrics.profit_factor {
        if profit_factor < thresholds.min_profit_factor {
            failures.push(JudgeReason::BelowProfitFactorThreshold {
                profit_factor,
                min: thresholds.min_profit_factor,
            });
        }
    }
    if metrics.expectancy <= Decimal::ZERO {
        failures.push(JudgeReason::NonPositiveExpectancy {
            expectancy: metrics.expectancy,
        });
    }
    if metrics.max_drawdown > thresholds.max_drawdown {
        failures.push(JudgeReason::ExcessiveDrawdown {
            drawdown: metrics.max_drawdown,
            max: thresholds.max_drawdown,
        });
    }

    if failures.is_empty() {
        return (JudgeState::Active, JudgeReason::MeetsAllThresholds);
    }

    let has_non_positive_expectancy = failures
        .iter()
        .any(|r| matches!(r, JudgeReason::NonPositiveExpectancy { .. }));

    if failures.len() == 1 && !has_non_positive_expectancy {
        (JudgeState::Degraded, failures[0])
    } else {
        let reason = failures
            .into_iter()
            .min_by_key(severity_rank)
            .expect("failures is non-empty here — the early return above covers the empty case");
        (JudgeState::Disabled, reason)
    }
}

fn severity_rank(reason: &JudgeReason) -> u8 {
    match reason {
        JudgeReason::NonPositiveExpectancy { .. } => 0,
        JudgeReason::BelowProfitFactorThreshold { .. } => 1,
        JudgeReason::ExcessiveDrawdown { .. } => 2,
        JudgeReason::BelowConsistencyThreshold { .. } => 3,
        JudgeReason::InsufficientSample { .. }
        | JudgeReason::MeetsAllThresholds
        | JudgeReason::TransitionPending { .. } => 4,
    }
}

fn uncertain_regime() -> RegimeAssessment {
    RegimeAssessment {
        regime: MarketRegime::Uncertain,
        strength: 0.0,
        rule: "no_market_features",
        summary: "sem features de mercado nesta avaliação".to_string(),
        evidence: RegimeEvidence {
            r_squared: None,
            slope: None,
            autocorrelation: None,
            bandwidth: None,
            prev_bandwidth: None,
            relative_volume: None,
            percent_b: None,
            zscore: None,
        },
    }
}

/// Avalia continuamente a viabilidade econômica de cada
/// `(StrategyId, InstrumentId)` a partir do histórico de trades fechados,
/// e recomenda qual estratégia deve operar — agora também podendo usar
/// o regime de mercado (Fase 2). **Recomenda, nunca decide sozinho**.
pub struct StrategyJudge {
    thresholds: JudgeThresholds,
    regime_thresholds: RegimeThresholds,
    memory: HashMap<(InstrumentId, StrategyId), Memory>,
    /// Chave = `context_id` (tipicamente `robot_id` — Fase A).
    selection_memory: HashMap<String, SelectionMemory>,
}

impl StrategyJudge {
    pub fn new(thresholds: JudgeThresholds) -> Self {
        Self {
            thresholds,
            regime_thresholds: RegimeThresholds::default(),
            memory: HashMap::new(),
            selection_memory: HashMap::new(),
        }
    }

    /// Avalia um único par, usando somente `positions` cujo `closed_at`
    /// já ocorreu em ou antes de `as_of` — nunca um trade fechado depois.
    pub fn evaluate(
        &mut self,
        strategy_id: &StrategyId,
        instrument_id: InstrumentId,
        positions: &[Position],
        as_of: DateTime<Utc>,
    ) -> JudgeDecision {
        let relevant: Vec<Position> = positions
            .iter()
            .filter(|p| {
                p.instrument_id == instrument_id
                    && &p.strategy_id == strategy_id
                    && p.status == PositionStatus::Closed
                    && p.closed_at.is_some_and(|closed_at| closed_at <= as_of)
            })
            .cloned()
            .collect();

        let report = analytics::compute_performance(&relevant);
        let metrics = JudgeMetrics {
            trades: report.total_trades,
            net_pnl: report.net_pnl,
            expectancy: report.expectancy,
            profit_factor: report.profit_factor,
            max_drawdown: report.max_drawdown,
            win_rate: report.win_rate,
        };

        let (proposed_state, proposed_reason) = classify(&metrics, &self.thresholds);
        let key = (instrument_id, strategy_id.clone());

        let (state, reason) = if let Some(memory) = self.memory.get_mut(&key) {
            if proposed_state == memory.state {
                memory.pending = None;
                (memory.state, proposed_reason)
            } else {
                let confirmations = match memory.pending {
                    Some((pending_state, count)) if pending_state == proposed_state => count + 1,
                    _ => 1,
                };
                if confirmations >= self.thresholds.min_confirmations {
                    memory.state = proposed_state;
                    memory.pending = None;
                    (proposed_state, proposed_reason)
                } else {
                    memory.pending = Some((proposed_state, confirmations));
                    (
                        memory.state,
                        JudgeReason::TransitionPending {
                            proposed: proposed_state,
                            confirmations,
                            required: self.thresholds.min_confirmations,
                        },
                    )
                }
            }
        } else {
            self.memory.insert(
                key,
                Memory {
                    state: proposed_state,
                    pending: None,
                },
            );
            (proposed_state, proposed_reason)
        };

        let decision = JudgeDecision {
            timestamp: as_of,
            strategy_id: strategy_id.clone(),
            instrument_id,
            state,
            metrics,
            reason,
        };

        match decision.state {
            JudgeState::Active => tracing::info!(
                strategy_id = %decision.strategy_id,
                instrument_id = ?decision.instrument_id,
                trades = decision.metrics.trades,
                net_pnl = %decision.metrics.net_pnl,
                "strategy judge: active"
            ),
            JudgeState::Degraded | JudgeState::Disabled => tracing::warn!(
                strategy_id = %decision.strategy_id,
                instrument_id = ?decision.instrument_id,
                state = ?decision.state,
                reason = ?decision.reason,
                "strategy judge: not fully healthy"
            ),
        }

        decision
    }

    /// Ranking econômico legado (sem features de mercado).
    pub fn recommend(
        &mut self,
        instrument_id: InstrumentId,
        candidates: &[StrategyId],
        positions: &[Position],
        as_of: DateTime<Utc>,
    ) -> (Vec<JudgeDecision>, Option<StrategyId>) {
        let outcome = self.recommend_contextual(
            "legacy",
            instrument_id,
            candidates,
            positions,
            as_of,
            None,
            None,
        );
        (outcome.decisions, outcome.selected)
    }

    /// Seleção contextual (Fase 2): regime + saúde econômica, escopada a
    /// `context_id` (robô). Só escolhe entre `candidates`.
    #[allow(clippy::too_many_arguments)]
    pub fn recommend_contextual(
        &mut self,
        context_id: &str,
        instrument_id: InstrumentId,
        candidates: &[StrategyId],
        positions: &[Position],
        as_of: DateTime<Utc>,
        market: Option<&FeatureSnapshot>,
        prev_bandwidth: Option<f64>,
    ) -> SelectionOutcome {
        let decisions: Vec<JudgeDecision> = candidates
            .iter()
            .map(|strategy_id| self.evaluate(strategy_id, instrument_id, positions, as_of))
            .collect();

        let regime = match market {
            Some(snap) => classify_regime(snap, prev_bandwidth, &self.regime_thresholds),
            None => uncertain_regime(),
        };

        let candidate_fits: Vec<CandidateFit> = decisions
            .iter()
            .map(|d| {
                let kind = strategy_kind_from_id(d.strategy_id.as_str()).to_string();
                CandidateFit {
                    strategy_id: d.strategy_id.clone(),
                    strategy_kind: kind.clone(),
                    fit_score: regime_affinity(regime.regime, &kind),
                    economic_state: d.state,
                }
            })
            .collect();

        let (raw_selected, raw_reason) = pick_raw_selection(&decisions, &candidate_fits, &regime);

        let (selected, selection_reason) = self.apply_selection_hysteresis(
            context_id,
            raw_selected,
            raw_reason,
            market.is_some() && regime.regime != MarketRegime::Uncertain,
        );

        SelectionOutcome {
            decisions,
            selected,
            regime,
            selection_reason,
            candidate_fits,
        }
    }

    fn apply_selection_hysteresis(
        &mut self,
        context_id: &str,
        proposed: Option<StrategyId>,
        proposed_reason: SelectionReason,
        use_hysteresis: bool,
    ) -> (Option<StrategyId>, SelectionReason) {
        if !use_hysteresis {
            self.selection_memory.insert(
                context_id.to_string(),
                SelectionMemory {
                    selected: proposed.clone(),
                    pending: None,
                },
            );
            return (proposed, proposed_reason);
        }

        let required = self.thresholds.min_confirmations;
        let entry = self
            .selection_memory
            .entry(context_id.to_string())
            .or_insert(SelectionMemory {
                selected: None,
                pending: None,
            });

        if entry.selected.is_none() && entry.pending.is_none() {
            entry.selected = proposed.clone();
            entry.pending = None;
            return (proposed, proposed_reason);
        }

        if proposed == entry.selected {
            entry.pending = None;
            return (entry.selected.clone(), proposed_reason);
        }

        let confirmations = match &entry.pending {
            Some((pending_sel, count)) if pending_sel == &proposed => count + 1,
            _ => 1,
        };

        if confirmations >= required {
            entry.selected = proposed.clone();
            entry.pending = None;
            (proposed, proposed_reason)
        } else {
            entry.pending = Some((proposed.clone(), confirmations));
            (
                entry.selected.clone(),
                SelectionReason::SelectionPending {
                    proposed,
                    confirmations,
                    required,
                    summary: format!(
                        "mudança de estratégia aguardando confirmação ({confirmations}/{required})"
                    ),
                },
            )
        }
    }
}

fn is_economically_blocked(decision: &JudgeDecision) -> bool {
    decision.state == JudgeState::Disabled
        && !matches!(decision.reason, JudgeReason::InsufficientSample { .. })
}

fn is_bootstrap_sample(decision: &JudgeDecision) -> bool {
    matches!(decision.reason, JudgeReason::InsufficientSample { .. })
}

fn pick_raw_selection(
    decisions: &[JudgeDecision],
    fits: &[CandidateFit],
    regime: &RegimeAssessment,
) -> (Option<StrategyId>, SelectionReason) {
    if decisions.is_empty() {
        return (
            None,
            SelectionReason::NoViableCandidate {
                summary: "nenhuma estratégia candidata configurada".to_string(),
            },
        );
    }

    if regime.regime == MarketRegime::Uncertain {
        // Mesmo critério de elegibilidade dos regimes claros: bloqueia só
        // inviabilidade econômica confirmada. `InsufficientSample` (bootstrap
        // sem trades) continua elegível — senão o robô ficaria sem estratégia
        // ativa até acumular amostra, o que impede o próprio bootstrap.
        //
        // Ranking: saúde econômica → afinidade Uncertain (generalistas na
        // frente) → StrategyId. Empate lexicográfico puro favorecia
        // `volatility_breakout`, que quase não entra quando o regime é
        // incerto — deadlock operacional (Judge escolhe, zero Longs).
        let best = decisions
            .iter()
            .filter(|d| !is_economically_blocked(d))
            .max_by(|a, b| {
                rank_by_economic_quality(a, b)
                    .then_with(|| {
                        let fa = fits
                            .iter()
                            .find(|f| f.strategy_id == a.strategy_id)
                            .map(|f| f.fit_score)
                            .unwrap_or(0.0);
                        let fb = fits
                            .iter()
                            .find(|f| f.strategy_id == b.strategy_id)
                            .map(|f| f.fit_score)
                            .unwrap_or(0.0);
                        fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .then_with(|| a.strategy_id.as_str().cmp(b.strategy_id.as_str()))
            });
        return match best {
            Some(d) => {
                let summary = if is_bootstrap_sample(d) {
                    format!(
                        "regime incerto ({}); bootstrap determinístico (afinidade + ordem estável) entre candidatas sem amostra suficiente",
                        regime.summary
                    )
                } else {
                    format!(
                        "regime incerto ({}); seleção pela qualidade econômica",
                        regime.summary
                    )
                };
                (
                    Some(d.strategy_id.clone()),
                    SelectionReason::EconomicFallback { summary },
                )
            }
            None => (
                None,
                SelectionReason::NoViableCandidate {
                    summary: format!(
                        "regime incerto ({}); nenhuma candidata economicamente viável",
                        regime.summary
                    ),
                },
            ),
        };
    }

    let eligible: Vec<&CandidateFit> = fits
        .iter()
        .filter(|fit| {
            decisions
                .iter()
                .find(|d| d.strategy_id == fit.strategy_id)
                .is_some_and(|d| !is_economically_blocked(d))
        })
        .collect();

    if eligible.is_empty() {
        return (
            None,
            SelectionReason::NoViableCandidate {
                summary: format!(
                    "regime {} mas nenhuma candidata economicamente viável",
                    regime.regime.as_str()
                ),
            },
        );
    }

    let best_fit = eligible
        .iter()
        .max_by(|a, b| {
            a.fit_score
                .partial_cmp(&b.fit_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    let da = decisions.iter().find(|d| d.strategy_id == a.strategy_id);
                    let db = decisions.iter().find(|d| d.strategy_id == b.strategy_id);
                    match (da, db) {
                        (Some(x), Some(y)) => rank_by_economic_quality(x, y),
                        _ => std::cmp::Ordering::Equal,
                    }
                })
        })
        .copied();

    let Some(winner) = best_fit else {
        return (
            None,
            SelectionReason::NoViableCandidate {
                summary: "falha interna ao ranquear afinidades".to_string(),
            },
        );
    };

    let decision = decisions
        .iter()
        .find(|d| d.strategy_id == winner.strategy_id)
        .expect("fit always references an evaluated decision");

    if is_bootstrap_sample(decision) {
        (
            Some(winner.strategy_id.clone()),
            SelectionReason::RegimeBootstrap {
                regime: regime.regime,
                strategy_kind: winner.strategy_kind.clone(),
                fit_score: winner.fit_score,
                summary: format!(
                    "{} selecionada no bootstrap (regime {}): {}",
                    winner.strategy_kind,
                    regime.regime.as_str(),
                    regime.summary
                ),
            },
        )
    } else {
        (
            Some(winner.strategy_id.clone()),
            SelectionReason::RegimeFit {
                regime: regime.regime,
                strategy_kind: winner.strategy_kind.clone(),
                fit_score: winner.fit_score,
                summary: format!(
                    "{} selecionada para regime {}: {}",
                    winner.strategy_kind,
                    regime.regime.as_str(),
                    regime.summary
                ),
            },
        )
    }
}

fn rank_by_economic_quality(a: &JudgeDecision, b: &JudgeDecision) -> std::cmp::Ordering {
    state_rank(a.state)
        .cmp(&state_rank(b.state))
        .then_with(|| a.metrics.expectancy.cmp(&b.metrics.expectancy))
        .then_with(|| {
            rank_profit_factor(a.metrics.profit_factor)
                .cmp(&rank_profit_factor(b.metrics.profit_factor))
        })
        .then_with(|| b.metrics.max_drawdown.cmp(&a.metrics.max_drawdown))
        .then_with(|| a.metrics.net_pnl.cmp(&b.metrics.net_pnl))
}

fn state_rank(state: JudgeState) -> u8 {
    match state {
        JudgeState::Disabled => 0,
        JudgeState::Degraded => 1,
        JudgeState::Active => 2,
    }
}

fn rank_profit_factor(profit_factor: Option<Decimal>) -> (u8, Decimal) {
    match profit_factor {
        Some(value) => (0, value),
        None => (1, Decimal::ZERO),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use features::{BollingerBands, RollingRegression};
    use rust_decimal_macros::dec;
    use std::collections::BTreeMap;
    use uuid::Uuid;

    fn closed_position(
        strategy: &str,
        instrument_id: InstrumentId,
        net_pnl: Decimal,
        closed_at: DateTime<Utc>,
    ) -> Position {
        Position {
            id: Uuid::new_v4(),
            instrument_id,
            strategy_id: StrategyId::new(strategy).unwrap(),
            side: domain::Side::Buy,
            quantity: dec!(1),
            entry_price: dec!(100),
            exit_price: Some(dec!(100) + net_pnl),
            opened_at: closed_at - Duration::minutes(1),
            closed_at: Some(closed_at),
            status: PositionStatus::Closed,
            realized_pnl_gross: Some(net_pnl),
            realized_pnl_net: Some(net_pnl),
            fees_paid: Decimal::ZERO,
            spread_paid: Decimal::ZERO,
            slippage_paid: Decimal::ZERO,
        }
    }

    fn trades(
        strategy: &str,
        instrument_id: InstrumentId,
        base: DateTime<Utc>,
        winners: usize,
        win_pnl: Decimal,
        losers: usize,
        loss_pnl: Decimal,
    ) -> Vec<Position> {
        let mut out = Vec::with_capacity(winners + losers);
        for i in 0..winners {
            out.push(closed_position(
                strategy,
                instrument_id,
                win_pnl,
                base + Duration::hours(i as i64),
            ));
        }
        for i in 0..losers {
            out.push(closed_position(
                strategy,
                instrument_id,
                -loss_pnl,
                base + Duration::hours((winners + i) as i64),
            ));
        }
        out
    }

    fn snap_with(
        regression: Option<RollingRegression>,
        autocorr: Option<f64>,
        bollinger: Option<BollingerBands>,
        rel_vol: Option<f64>,
    ) -> FeatureSnapshot {
        FeatureSnapshot {
            instrument_id: InstrumentId::new(),
            timestamp: Utc::now(),
            close: 100.0,
            returns: BTreeMap::new(),
            sma: Some(100.0),
            ema: Some(100.0),
            stddev: Some(1.0),
            realized_volatility: Some(0.01),
            ewma_volatility: Some(0.01),
            zscore: Some(0.1),
            bollinger,
            atr: Some(1.0),
            rsi: Some(50.0),
            roc: Some(0.0),
            regression,
            autocorrelation: autocorr,
            relative_volume: rel_vol.or(Some(1.0)),
        }
    }

    #[test]
    fn recommends_the_strategy_with_the_highest_net_pnl() {
        let instrument = InstrumentId::new();
        let base = Utc::now();
        let mut positions = trades("robot-a", instrument, base, 7, dec!(100), 3, dec!(30));
        positions.extend(trades(
            "robot-b",
            instrument,
            base,
            6,
            dec!(50),
            4,
            dec!(20),
        ));
        let as_of = base + Duration::hours(20);

        let mut judge = StrategyJudge::new(JudgeThresholds::default());
        let candidates = vec![
            StrategyId::new("robot-a").unwrap(),
            StrategyId::new("robot-b").unwrap(),
        ];
        let (decisions, best) = judge.recommend(instrument, &candidates, &positions, as_of);

        assert_eq!(decisions.len(), 2);
        assert!(decisions.iter().all(|d| d.state == JudgeState::Active));
        assert_eq!(best, Some(StrategyId::new("robot-a").unwrap()));
    }

    #[test]
    fn higher_net_pnl_does_not_win_when_expectancy_is_worse() {
        let instrument = InstrumentId::new();
        let base = Utc::now();
        let positions_a = trades("robot-a", instrument, base, 100, dec!(1), 0, dec!(0));
        let positions_b = trades("robot-b", instrument, base, 10, dec!(5), 0, dec!(0));
        let mut positions = positions_a;
        positions.extend(positions_b);
        let as_of = base + Duration::hours(200);

        let mut judge = StrategyJudge::new(JudgeThresholds::default());
        let candidates = vec![
            StrategyId::new("robot-a").unwrap(),
            StrategyId::new("robot-b").unwrap(),
        ];
        let (_decisions, best) = judge.recommend(instrument, &candidates, &positions, as_of);
        assert_eq!(best, Some(StrategyId::new("robot-b").unwrap()));
    }

    #[test]
    fn higher_net_pnl_does_not_win_when_drawdown_is_worse() {
        let instrument = InstrumentId::new();
        let base = Utc::now();
        let strategy_a = "robot-a";
        let strategy_b = "robot-b";

        let mut positions = vec![
            closed_position(strategy_a, instrument, dec!(-100), base),
            closed_position(
                strategy_a,
                instrument,
                dec!(-100),
                base + Duration::hours(1),
            ),
            closed_position(strategy_a, instrument, dec!(100), base + Duration::hours(2)),
            closed_position(strategy_a, instrument, dec!(100), base + Duration::hours(3)),
            closed_position(strategy_a, instrument, dec!(100), base + Duration::hours(4)),
            closed_position(strategy_a, instrument, dec!(100), base + Duration::hours(5)),
        ];
        positions.extend(vec![
            closed_position(strategy_b, instrument, dec!(100), base),
            closed_position(
                strategy_b,
                instrument,
                dec!(-100),
                base + Duration::hours(1),
            ),
            closed_position(strategy_b, instrument, dec!(100), base + Duration::hours(2)),
            closed_position(
                strategy_b,
                instrument,
                dec!(-100),
                base + Duration::hours(3),
            ),
            closed_position(strategy_b, instrument, dec!(100), base + Duration::hours(4)),
            closed_position(strategy_b, instrument, dec!(100), base + Duration::hours(5)),
        ]);
        let as_of = base + Duration::hours(6);

        let thresholds = JudgeThresholds {
            min_sample_size: 6,
            ..JudgeThresholds::default()
        };
        let mut judge = StrategyJudge::new(thresholds);
        let candidates = vec![
            StrategyId::new(strategy_a).unwrap(),
            StrategyId::new(strategy_b).unwrap(),
        ];
        let (_decisions, best) = judge.recommend(instrument, &candidates, &positions, as_of);
        assert_eq!(best, Some(StrategyId::new(strategy_b).unwrap()));
    }

    #[test]
    fn active_beats_disabled_regardless_of_net_pnl() {
        let instrument = InstrumentId::new();
        let base = Utc::now();
        let positions_a = trades("robot-a", instrument, base, 3, dec!(10_000), 0, dec!(0));
        let mut positions = positions_a;
        positions.extend(trades(
            "robot-b",
            instrument,
            base,
            10,
            dec!(10),
            0,
            dec!(0),
        ));
        let as_of = base + Duration::hours(20);

        let mut judge = StrategyJudge::new(JudgeThresholds::default());
        let candidates = vec![
            StrategyId::new("robot-a").unwrap(),
            StrategyId::new("robot-b").unwrap(),
        ];
        let (_decisions, best) = judge.recommend(instrument, &candidates, &positions, as_of);
        assert_eq!(best, Some(StrategyId::new("robot-b").unwrap()));
    }

    #[test]
    fn insufficient_sample_is_always_disabled() {
        let instrument = InstrumentId::new();
        let base = Utc::now();
        let positions = trades("robot-a", instrument, base, 3, dec!(100), 0, dec!(0));

        let mut judge = StrategyJudge::new(JudgeThresholds::default());
        let decision = judge.evaluate(
            &StrategyId::new("robot-a").unwrap(),
            instrument,
            &positions,
            base + Duration::hours(5),
        );

        assert_eq!(decision.state, JudgeState::Disabled);
        assert_eq!(
            decision.reason,
            JudgeReason::InsufficientSample {
                trades: 3,
                required: 10
            }
        );
    }

    #[test]
    fn degrades_when_exactly_one_criterion_fails() {
        let instrument = InstrumentId::new();
        let base = Utc::now();
        let positions = trades("robot-a", instrument, base, 3, dec!(200), 7, dec!(10));

        let mut judge = StrategyJudge::new(JudgeThresholds::default());
        let decision = judge.evaluate(
            &StrategyId::new("robot-a").unwrap(),
            instrument,
            &positions,
            base + Duration::hours(11),
        );

        assert_eq!(decision.state, JudgeState::Degraded);
    }

    #[test]
    fn recommendation_switches_as_history_accumulates() {
        let instrument = InstrumentId::new();
        let base = Utc::now();

        let mut positions = trades("robot-a", instrument, base, 20, dec!(10), 0, dec!(0));
        for i in 0..10 {
            positions.push(closed_position(
                "robot-b",
                instrument,
                dec!(5),
                base + Duration::hours(i),
            ));
        }
        for i in 10..20 {
            positions.push(closed_position(
                "robot-b",
                instrument,
                dec!(50),
                base + Duration::hours(i),
            ));
        }

        let candidates = vec![
            StrategyId::new("robot-a").unwrap(),
            StrategyId::new("robot-b").unwrap(),
        ];
        let mut judge = StrategyJudge::new(JudgeThresholds::default());

        let (_early_decisions, early_best) = judge.recommend(
            instrument,
            &candidates,
            &positions,
            base + Duration::hours(9),
        );
        assert_eq!(early_best, Some(StrategyId::new("robot-a").unwrap()));

        let (_late_decisions, late_best) = judge.recommend(
            instrument,
            &candidates,
            &positions,
            base + Duration::hours(19),
        );
        assert_eq!(late_best, Some(StrategyId::new("robot-b").unwrap()));
    }

    #[test]
    fn does_not_flip_state_until_the_change_is_confirmed_repeatedly() {
        let instrument = InstrumentId::new();
        let strategy = StrategyId::new("robot-a").unwrap();
        let base = Utc::now();
        let thresholds = JudgeThresholds {
            min_sample_size: 1,
            min_win_rate: dec!(0.5),
            min_profit_factor: Decimal::ZERO,
            max_drawdown: dec!(1_000_000),
            min_confirmations: 3,
        };
        let mut judge = StrategyJudge::new(thresholds);

        let outcomes = [
            dec!(100),
            dec!(100),
            dec!(-10),
            dec!(-10),
            dec!(-10),
            dec!(100),
            dec!(-10),
            dec!(-10),
            dec!(-10),
        ];
        let positions: Vec<Position> = outcomes
            .iter()
            .enumerate()
            .map(|(i, pnl)| {
                closed_position(
                    "robot-a",
                    instrument,
                    *pnl,
                    base + Duration::hours(i as i64),
                )
            })
            .collect();

        let eval_at = |judge: &mut StrategyJudge, upto_index: usize| {
            judge.evaluate(
                &strategy,
                instrument,
                &positions,
                base + Duration::hours(upto_index as i64),
            )
        };

        for i in 0..4 {
            assert_eq!(eval_at(&mut judge, i).state, JudgeState::Active);
        }

        let d4 = eval_at(&mut judge, 4);
        assert_eq!(d4.state, JudgeState::Active);
        assert!(matches!(d4.reason, JudgeReason::TransitionPending { .. }));

        let d5 = eval_at(&mut judge, 5);
        assert_eq!(d5.state, JudgeState::Active);

        let d6 = eval_at(&mut judge, 6);
        assert_eq!(d6.state, JudgeState::Active);
        let d7 = eval_at(&mut judge, 7);
        assert_eq!(d7.state, JudgeState::Active);

        let d8 = eval_at(&mut judge, 8);
        assert_eq!(d8.state, JudgeState::Degraded);
    }

    #[test]
    fn decision_is_unaffected_by_trades_closed_after_as_of() {
        let instrument = InstrumentId::new();
        let strategy = StrategyId::new("robot-a").unwrap();
        let base = Utc::now();

        let mut all_positions = trades("robot-a", instrument, base, 6, dec!(50), 4, dec!(20));
        for i in 0..10 {
            all_positions.push(closed_position(
                "robot-a",
                instrument,
                dec!(-1000),
                base + Duration::hours(20 + i),
            ));
        }

        let as_of = base + Duration::hours(11);
        let prefix_only: Vec<Position> = all_positions
            .iter()
            .filter(|p| p.closed_at.unwrap() <= as_of)
            .cloned()
            .collect();

        let mut judge_prefix = StrategyJudge::new(JudgeThresholds::default());
        let mut judge_full = StrategyJudge::new(JudgeThresholds::default());

        let from_prefix = judge_prefix.evaluate(&strategy, instrument, &prefix_only, as_of);
        let from_full = judge_full.evaluate(&strategy, instrument, &all_positions, as_of);

        assert_eq!(from_prefix, from_full);
        assert_eq!(from_prefix.metrics.trades, 10);
        assert_eq!(from_prefix.state, JudgeState::Active);
    }

    #[test]
    fn profit_factor_none_never_fails_the_criterion() {
        let instrument = InstrumentId::new();
        let base = Utc::now();
        let positions = trades("robot-a", instrument, base, 10, dec!(50), 0, dec!(0));

        let mut judge = StrategyJudge::new(JudgeThresholds::default());
        let decision = judge.evaluate(
            &StrategyId::new("robot-a").unwrap(),
            instrument,
            &positions,
            base + Duration::hours(10),
        );

        assert_eq!(decision.state, JudgeState::Active);
        assert_eq!(decision.metrics.profit_factor, None);
    }

    #[test]
    fn comparing_only_within_the_same_instrument() {
        let instrument_a = InstrumentId::new();
        let instrument_b = InstrumentId::new();
        let base = Utc::now();
        let mut positions = trades("robot-a", instrument_a, base, 10, dec!(100), 0, dec!(0));
        positions.extend(trades(
            "robot-a",
            instrument_b,
            base,
            0,
            dec!(0),
            10,
            dec!(100),
        ));

        let mut judge = StrategyJudge::new(JudgeThresholds::default());
        let strategy = StrategyId::new("robot-a").unwrap();
        let as_of = base + Duration::hours(10);

        let decision_a = judge.evaluate(&strategy, instrument_a, &positions, as_of);
        let decision_b = judge.evaluate(&strategy, instrument_b, &positions, as_of);

        assert_eq!(decision_a.state, JudgeState::Active);
        assert_eq!(decision_a.metrics.net_pnl, dec!(1000));
        assert_eq!(decision_b.metrics.net_pnl, dec!(-1000));
    }

    #[test]
    fn trending_market_selects_momentum_among_candidates() {
        let instrument = InstrumentId::new();
        let base = Utc::now();
        let candidates = vec![
            StrategyId::new("r::momentum").unwrap(),
            StrategyId::new("r::mean_reversion").unwrap(),
            StrategyId::new("r::ema_crossover").unwrap(),
        ];
        let market = snap_with(
            Some(RollingRegression {
                slope: 0.4,
                intercept: 90.0,
                r_squared: 0.8,
            }),
            Some(0.5),
            None,
            None,
        );

        let mut judge = StrategyJudge::new(JudgeThresholds::default());
        let outcome = judge.recommend_contextual(
            "robot-a",
            instrument,
            &candidates,
            &[],
            base,
            Some(&market),
            None,
        );

        assert_eq!(outcome.regime.regime, MarketRegime::Trending);
        assert_eq!(
            outcome.selected,
            Some(StrategyId::new("r::momentum").unwrap())
        );
        assert!(matches!(
            outcome.selection_reason,
            SelectionReason::RegimeBootstrap { .. }
        ));
    }

    #[test]
    fn ranging_market_selects_mean_reversion() {
        let instrument = InstrumentId::new();
        let candidates = vec![
            StrategyId::new("r::momentum").unwrap(),
            StrategyId::new("r::statistical_mean_reversion").unwrap(),
        ];
        let market = snap_with(
            Some(RollingRegression {
                slope: 0.0,
                intercept: 100.0,
                r_squared: 0.1,
            }),
            Some(0.05),
            None,
            None,
        );

        let mut judge = StrategyJudge::new(JudgeThresholds::default());
        let outcome = judge.recommend_contextual(
            "robot-a",
            instrument,
            &candidates,
            &[],
            Utc::now(),
            Some(&market),
            None,
        );

        assert_eq!(outcome.regime.regime, MarketRegime::Ranging);
        assert_eq!(
            outcome.selected,
            Some(StrategyId::new("r::statistical_mean_reversion").unwrap())
        );
    }

    #[test]
    fn breakout_market_selects_volatility_breakout() {
        let instrument = InstrumentId::new();
        let candidates = vec![
            StrategyId::new("r::mean_reversion").unwrap(),
            StrategyId::new("r::volatility_breakout").unwrap(),
            StrategyId::new("r::momentum").unwrap(),
        ];
        let market = snap_with(
            Some(RollingRegression {
                slope: 0.2,
                intercept: 100.0,
                r_squared: 0.4,
            }),
            Some(0.1),
            Some(BollingerBands {
                middle: 100.0,
                upper: 102.0,
                lower: 98.0,
                percent_b: 1.25,
                bandwidth: 0.09,
            }),
            Some(1.8),
        );

        let mut judge = StrategyJudge::new(JudgeThresholds::default());
        let outcome = judge.recommend_contextual(
            "robot-a",
            instrument,
            &candidates,
            &[],
            Utc::now(),
            Some(&market),
            Some(0.04),
        );

        assert_eq!(outcome.regime.regime, MarketRegime::VolatilityExpansion);
        assert_eq!(
            outcome.selected,
            Some(StrategyId::new("r::volatility_breakout").unwrap())
        );
    }

    #[test]
    fn selection_respects_candidate_list_only() {
        let instrument = InstrumentId::new();
        // Sem momentum nas candidatas — trending deve cair em ema.
        let candidates = vec![
            StrategyId::new("r::mean_reversion").unwrap(),
            StrategyId::new("r::ema_crossover").unwrap(),
        ];
        let market = snap_with(
            Some(RollingRegression {
                slope: 0.5,
                intercept: 90.0,
                r_squared: 0.9,
            }),
            Some(0.6),
            None,
            None,
        );

        let mut judge = StrategyJudge::new(JudgeThresholds::default());
        let outcome = judge.recommend_contextual(
            "robot-a",
            instrument,
            &candidates,
            &[],
            Utc::now(),
            Some(&market),
            None,
        );

        assert_eq!(
            outcome.selected,
            Some(StrategyId::new("r::ema_crossover").unwrap())
        );
        assert!(!outcome
            .candidate_fits
            .iter()
            .any(|f| f.strategy_kind == "momentum"));
    }

    #[test]
    fn two_context_ids_keep_independent_selections() {
        let instrument = InstrumentId::new();
        let trending = snap_with(
            Some(RollingRegression {
                slope: 0.5,
                intercept: 90.0,
                r_squared: 0.85,
            }),
            Some(0.5),
            None,
            None,
        );
        let ranging = snap_with(
            Some(RollingRegression {
                slope: 0.0,
                intercept: 100.0,
                r_squared: 0.1,
            }),
            Some(0.0),
            None,
            None,
        );
        let candidates = vec![
            StrategyId::new("a::momentum").unwrap(),
            StrategyId::new("a::mean_reversion").unwrap(),
        ];
        let candidates_b = vec![
            StrategyId::new("b::momentum").unwrap(),
            StrategyId::new("b::mean_reversion").unwrap(),
        ];

        let mut judge = StrategyJudge::new(JudgeThresholds::default());
        let out_a = judge.recommend_contextual(
            "robot-a",
            instrument,
            &candidates,
            &[],
            Utc::now(),
            Some(&trending),
            None,
        );
        let out_b = judge.recommend_contextual(
            "robot-b",
            instrument,
            &candidates_b,
            &[],
            Utc::now(),
            Some(&ranging),
            None,
        );

        assert_eq!(
            out_a.selected,
            Some(StrategyId::new("a::momentum").unwrap())
        );
        assert_eq!(
            out_b.selected,
            Some(StrategyId::new("b::mean_reversion").unwrap())
        );
    }

    #[test]
    fn regime_selection_does_not_flip_on_single_observation() {
        let instrument = InstrumentId::new();
        let thresholds = JudgeThresholds {
            min_confirmations: 3,
            ..JudgeThresholds::default()
        };
        let mut judge = StrategyJudge::new(thresholds);
        let candidates = vec![
            StrategyId::new("r::momentum").unwrap(),
            StrategyId::new("r::mean_reversion").unwrap(),
        ];
        let trending = snap_with(
            Some(RollingRegression {
                slope: 0.5,
                intercept: 90.0,
                r_squared: 0.85,
            }),
            Some(0.5),
            None,
            None,
        );
        let ranging = snap_with(
            Some(RollingRegression {
                slope: 0.0,
                intercept: 100.0,
                r_squared: 0.1,
            }),
            Some(0.0),
            None,
            None,
        );

        let first = judge.recommend_contextual(
            "robot-a",
            instrument,
            &candidates,
            &[],
            Utc::now(),
            Some(&trending),
            None,
        );
        assert_eq!(
            first.selected,
            Some(StrategyId::new("r::momentum").unwrap())
        );

        // Uma única observação ranging não deve trocar ainda.
        let second = judge.recommend_contextual(
            "robot-a",
            instrument,
            &candidates,
            &[],
            Utc::now(),
            Some(&ranging),
            None,
        );
        assert_eq!(
            second.selected,
            Some(StrategyId::new("r::momentum").unwrap())
        );
        assert!(matches!(
            second.selection_reason,
            SelectionReason::SelectionPending {
                confirmations: 1,
                ..
            }
        ));
    }

    #[test]
    fn selection_reason_matches_classified_regime() {
        let instrument = InstrumentId::new();
        let candidates = vec![StrategyId::new("r::momentum").unwrap()];
        let market = snap_with(
            Some(RollingRegression {
                slope: 0.4,
                intercept: 90.0,
                r_squared: 0.8,
            }),
            Some(0.5),
            None,
            None,
        );
        let mut judge = StrategyJudge::new(JudgeThresholds::default());
        let outcome = judge.recommend_contextual(
            "robot-a",
            instrument,
            &candidates,
            &[],
            Utc::now(),
            Some(&market),
            None,
        );
        match &outcome.selection_reason {
            SelectionReason::RegimeBootstrap {
                regime, summary, ..
            }
            | SelectionReason::RegimeFit {
                regime, summary, ..
            } => {
                assert_eq!(*regime, MarketRegime::Trending);
                assert!(
                    summary.contains("tendência")
                        || summary.contains("trending")
                        || summary.contains("momentum")
                );
            }
            other => panic!("unexpected reason: {other:?}"),
        }
    }

    /// Cenário real de `btc-all`: candles + regime Uncertain (sinais mistos),
    /// histórico vazio → todas Disabled/InsufficientSample. Ainda assim o
    /// Judge deve escolher uma candidata (fallback econômico determinístico),
    /// sem exigir `Active` nem forçar trade.
    #[test]
    fn uncertain_bootstrap_selects_deterministic_economic_fallback() {
        let instrument = InstrumentId::new();
        let candidates = vec![
            StrategyId::new("btc-all::ema_crossover").unwrap(),
            StrategyId::new("btc-all::momentum").unwrap(),
            StrategyId::new("btc-all::mean_reversion").unwrap(),
            StrategyId::new("btc-all::statistical_mean_reversion").unwrap(),
            StrategyId::new("btc-all::quant_momentum").unwrap(),
            StrategyId::new("btc-all::volatility_breakout").unwrap(),
        ];
        // R² intermediário + autocorr negativa: nem trend limpo nem ranging.
        let market = snap_with(
            Some(RollingRegression {
                slope: -3.5,
                intercept: 100.0,
                r_squared: 0.39,
            }),
            Some(-0.27),
            Some(BollingerBands {
                middle: 100.0,
                upper: 100.1,
                lower: 99.9,
                percent_b: 0.16,
                bandwidth: 0.0017,
            }),
            Some(5.0),
        );

        let mut judge = StrategyJudge::new(JudgeThresholds::default());
        let outcome = judge.recommend_contextual(
            "btc-all",
            instrument,
            &candidates,
            &[],
            Utc::now(),
            Some(&market),
            Some(0.0016),
        );

        assert_eq!(outcome.regime.regime, MarketRegime::Uncertain);
        assert!(
            outcome.decisions.iter().all(|d| {
                d.state == JudgeState::Disabled
                    && matches!(d.reason, JudgeReason::InsufficientSample { .. })
            }),
            "bootstrap: todas InsufficientSample"
        );
        assert_eq!(
            outcome.selected,
            Some(StrategyId::new("btc-all::ema_crossover").unwrap()),
            "Uncertain bootstrap deve preferir generalista (ema), não breakout"
        );
        assert!(matches!(
            outcome.selection_reason,
            SelectionReason::EconomicFallback { .. }
        ));

        // Determinismo: segunda avaliação idêntica não muda a escolha.
        let again = judge.recommend_contextual(
            "btc-all",
            instrument,
            &candidates,
            &[],
            Utc::now(),
            Some(&market),
            Some(0.0016),
        );
        assert_eq!(again.selected, outcome.selected);
    }

    #[test]
    fn uncertain_prefers_active_over_bootstrap_when_history_exists() {
        let instrument = InstrumentId::new();
        let base = Utc::now();
        let candidates = vec![
            StrategyId::new("r::ema_crossover").unwrap(),
            StrategyId::new("r::momentum").unwrap(),
        ];
        let positions = trades("r::ema_crossover", instrument, base, 10, dec!(10), 0, dec!(0));
        let market = snap_with(
            Some(RollingRegression {
                slope: 0.1,
                intercept: 100.0,
                r_squared: 0.4,
            }),
            Some(-0.1),
            None,
            None,
        );

        let mut judge = StrategyJudge::new(JudgeThresholds::default());
        let outcome = judge.recommend_contextual(
            "robot-a",
            instrument,
            &candidates,
            &positions,
            base + Duration::hours(20),
            Some(&market),
            None,
        );

        assert_eq!(outcome.regime.regime, MarketRegime::Uncertain);
        assert_eq!(
            outcome.selected,
            Some(StrategyId::new("r::ema_crossover").unwrap())
        );
        let ema = outcome
            .decisions
            .iter()
            .find(|d| d.strategy_id.as_str() == "r::ema_crossover")
            .unwrap();
        assert_eq!(ema.state, JudgeState::Active);
    }

    #[test]
    fn uncertain_with_only_confirmed_disabled_selects_none() {
        let instrument = InstrumentId::new();
        let base = Utc::now();
        let candidates = vec![StrategyId::new("r::momentum").unwrap()];
        // Amostra suficiente com expectancy negativa → Disabled real.
        let positions = trades("r::momentum", instrument, base, 2, dec!(1), 8, dec!(10));
        let mut judge = StrategyJudge::new(JudgeThresholds::default());
        let outcome = judge.recommend_contextual(
            "robot-a",
            instrument,
            &candidates,
            &positions,
            base + Duration::hours(20),
            None,
            None,
        );

        assert_eq!(outcome.regime.regime, MarketRegime::Uncertain);
        assert_eq!(outcome.selected, None);
        assert!(matches!(
            outcome.selection_reason,
            SelectionReason::NoViableCandidate { .. }
        ));
    }
}
