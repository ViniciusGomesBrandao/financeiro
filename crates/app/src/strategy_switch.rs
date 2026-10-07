//! Integra `strategy_judge::StrategyJudge` ao pipeline live/paper (Fase
//! 3 / Fase A / Judge contextual Fase 2): decide, **por robô**, a cada
//! candle, qual estratégia deve estar ativa — usando regime de mercado
//! quando houver features — e o que precisa acontecer quando essa escolha
//! muda. **Decide, nunca executa**.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use domain::{InstrumentId, Position, PositionStatus, Signal, SignalDirection, StrategyId};
use features::FeatureSnapshot;
use strategy_judge::{
    JudgeDecision, JudgeReason, JudgeState, RegimeAssessment, SelectionOutcome, SelectionReason,
    StrategyJudge,
};

/// Uma mudança de estratégia ativa detectada para um instrumento.
#[derive(Debug, Clone, PartialEq)]
pub struct StrategySwitch {
    pub instrument_id: InstrumentId,
    pub previous: Option<StrategyId>,
    pub new: Option<StrategyId>,
    pub timestamp: DateTime<Utc>,
    pub reason: JudgeReason,
}

/// O resultado de reavaliar um robô num instante.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveStrategyUpdate {
    pub instrument_id: InstrumentId,
    pub selected: Option<StrategyId>,
    pub decisions: Vec<JudgeDecision>,
    pub positions_to_close: Vec<Position>,
    pub switch: Option<StrategySwitch>,
    pub regime: RegimeAssessment,
    pub selection_reason: SelectionReason,
    pub candidate_fits: Vec<strategy_judge::CandidateFit>,
}

/// Mantém, por **robô**, a última recomendação conhecida do Judge.
pub struct ActiveStrategyTracker {
    judge: StrategyJudge,
    last_selected: HashMap<String, Option<StrategyId>>,
}

impl ActiveStrategyTracker {
    pub fn new(judge: StrategyJudge) -> Self {
        Self {
            judge,
            last_selected: HashMap::new(),
        }
    }

    /// Reavalia as candidatas **deste robô**, com features opcionais.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        robot_id: &str,
        instrument_id: InstrumentId,
        candidates: &[StrategyId],
        trade_history: &[Position],
        open_positions: &[Position],
        as_of: DateTime<Utc>,
        market: Option<&FeatureSnapshot>,
        prev_bandwidth: Option<f64>,
    ) -> ActiveStrategyUpdate {
        let SelectionOutcome {
            decisions,
            selected,
            regime,
            selection_reason,
            candidate_fits,
        } = self.judge.recommend_contextual(
            robot_id,
            instrument_id,
            candidates,
            trade_history,
            as_of,
            market,
            prev_bandwidth,
        );

        let positions_to_close: Vec<Position> = open_positions
            .iter()
            .filter(|p| {
                p.instrument_id == instrument_id
                    && p.status == PositionStatus::Open
                    && candidates.iter().any(|c| c == &p.strategy_id)
                    && Some(&p.strategy_id) != selected.as_ref()
            })
            .cloned()
            .collect();

        let previous = self
            .last_selected
            .insert(robot_id.to_string(), selected.clone());
        let switch = match previous {
            None => None,
            Some(prev) if prev == selected => None,
            Some(prev) => {
                let reason_owner = selected.clone().or_else(|| prev.clone());
                let reason = reason_owner
                    .and_then(|id| decisions.iter().find(|d| d.strategy_id == id))
                    .map(|d| d.reason)
                    .unwrap_or(JudgeReason::MeetsAllThresholds);
                Some(StrategySwitch {
                    instrument_id,
                    previous: prev,
                    new: selected.clone(),
                    timestamp: as_of,
                    reason,
                })
            }
        };

        ActiveStrategyUpdate {
            instrument_id,
            selected,
            decisions,
            positions_to_close,
            switch,
            regime,
            selection_reason,
            candidate_fits,
        }
    }
}

/// Um sinal `Long` só é permitido quando vem da estratégia selecionada —
/// **exceto no bootstrap** (todas / a selecionada ainda em
/// `InsufficientSample`): aí qualquer candidata não bloqueada
/// economicamente pode abrir, senão o Judge escolhe p.ex. `ema_crossover`
/// e o robô fica sem trades enquanto `mean_reversion` geraria Longs.
pub fn signal_allowed(update: &ActiveStrategyUpdate, signal: &Signal) -> bool {
    if !matches!(signal.direction, SignalDirection::Long) {
        return true;
    }

    let decision_for =
        |id: &StrategyId| update.decisions.iter().find(|d| &d.strategy_id == id);

    let is_bootstrap = |d: &JudgeDecision| {
        matches!(d.reason, JudgeReason::InsufficientSample { .. })
    };

    let selected_in_bootstrap = update
        .selected
        .as_ref()
        .and_then(decision_for)
        .is_some_and(is_bootstrap);

    let all_in_bootstrap = !update.decisions.is_empty()
        && update.decisions.iter().all(is_bootstrap);

    // Sem amostra ainda: não restringe à "preferida" do Judge — precisa
    // gerar os primeiros trades para o próprio Judge avaliar.
    if update.selected.is_none() || selected_in_bootstrap || all_in_bootstrap {
        return match decision_for(&signal.strategy_id) {
            Some(decision) if decision.state == JudgeState::Disabled => is_bootstrap(decision),
            _ => true,
        };
    }

    update
        .selected
        .as_ref()
        .is_some_and(|selected| selected == &signal.strategy_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use strategy_judge::JudgeThresholds;
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

    fn open_position(
        strategy: &str,
        instrument_id: InstrumentId,
        opened_at: DateTime<Utc>,
    ) -> Position {
        Position {
            id: Uuid::new_v4(),
            instrument_id,
            strategy_id: StrategyId::new(strategy).unwrap(),
            side: domain::Side::Buy,
            quantity: dec!(1),
            entry_price: dec!(100),
            exit_price: None,
            opened_at,
            closed_at: None,
            status: PositionStatus::Open,
            realized_pnl_gross: None,
            realized_pnl_net: None,
            fees_paid: Decimal::ZERO,
            spread_paid: Decimal::ZERO,
            slippage_paid: Decimal::ZERO,
        }
    }

    fn wins(
        strategy: &str,
        instrument_id: InstrumentId,
        base: DateTime<Utc>,
        count: usize,
        pnl: Decimal,
    ) -> Vec<Position> {
        (0..count)
            .map(|i| {
                closed_position(
                    strategy,
                    instrument_id,
                    pnl,
                    base + Duration::hours(i as i64),
                )
            })
            .collect()
    }

    fn signal(strategy: &str, instrument_id: InstrumentId, direction: SignalDirection) -> Signal {
        Signal::new(
            StrategyId::new(strategy).unwrap(),
            instrument_id,
            direction,
            0.8,
            Utc::now(),
            None,
            None,
            serde_json::Value::Null,
        )
        .unwrap()
    }

    fn update_with(
        instrument_id: InstrumentId,
        selected: Option<&str>,
        decisions: Vec<JudgeDecision>,
    ) -> ActiveStrategyUpdate {
        use strategy_judge::{MarketRegime, RegimeAssessment, RegimeEvidence, SelectionReason};
        ActiveStrategyUpdate {
            instrument_id,
            selected: selected.map(|s| StrategyId::new(s).unwrap()),
            decisions,
            positions_to_close: Vec::new(),
            switch: None,
            regime: RegimeAssessment {
                regime: MarketRegime::Uncertain,
                strength: 0.0,
                rule: "test",
                summary: "test".into(),
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
            },
            selection_reason: SelectionReason::EconomicFallback {
                summary: "test".into(),
            },
            candidate_fits: Vec::new(),
        }
    }

    fn decision(strategy: &str, state: JudgeState, reason: JudgeReason) -> JudgeDecision {
        JudgeDecision {
            timestamp: Utc::now(),
            strategy_id: StrategyId::new(strategy).unwrap(),
            instrument_id: InstrumentId::new(),
            state,
            metrics: strategy_judge::JudgeMetrics {
                trades: 0,
                net_pnl: Decimal::ZERO,
                expectancy: Decimal::ZERO,
                profit_factor: None,
                max_drawdown: Decimal::ZERO,
                win_rate: Decimal::ZERO,
            },
            reason,
        }
    }

    /// Requisito 1: só a estratégia selecionada pode abrir posição.
    #[test]
    fn only_the_selected_strategy_may_open_a_position() {
        let instrument = InstrumentId::new();
        let update = update_with(instrument, Some("robot-a"), Vec::new());

        assert!(signal_allowed(
            &update,
            &signal("robot-a", instrument, SignalDirection::Long)
        ));
        assert!(!signal_allowed(
            &update,
            &signal("robot-b", instrument, SignalDirection::Long)
        ));
        // Short/Flat só fecham posição existente — sempre passam,
        // independentemente de quem está selecionado.
        assert!(signal_allowed(
            &update,
            &signal("robot-b", instrument, SignalDirection::Short)
        ));
        assert!(signal_allowed(
            &update,
            &signal("robot-b", instrument, SignalDirection::Flat)
        ));

        let none_selected = update_with(instrument, None, Vec::new());
        assert!(signal_allowed(
            &none_selected,
            &signal("robot-a", instrument, SignalDirection::Flat)
        ));
    }

    /// Sem vencedor ainda (`selected = None`) por falta de amostra
    /// (bootstrap: histórico vazio) — sinais `Long` continuam permitidos,
    /// senão nenhuma estratégia jamais abriria a primeira posição para
    /// gerar o histórico que o Judge precisa.
    #[test]
    fn insufficient_sample_never_blocks_a_signal_when_no_one_is_selected_yet() {
        let instrument = InstrumentId::new();
        let decisions = vec![decision(
            "robot-a",
            JudgeState::Disabled,
            JudgeReason::InsufficientSample {
                trades: 0,
                required: 10,
            },
        )];
        let update = update_with(instrument, None, decisions);

        assert!(signal_allowed(
            &update,
            &signal("robot-a", instrument, SignalDirection::Long)
        ));
    }

    /// O Judge contextual escolhe uma candidata mesmo no bootstrap
    /// (`selected = Some`), mas isso é só ranking de afinidade — ainda
    /// sem trades. Outras estratégias com `InsufficientSample` devem
    /// poder abrir Long; senão o robô trava sem ordens.
    #[test]
    fn bootstrap_selected_strategy_does_not_block_other_longs() {
        let instrument = InstrumentId::new();
        let decisions = vec![
            decision(
                "ema_crossover",
                JudgeState::Disabled,
                JudgeReason::InsufficientSample {
                    trades: 0,
                    required: 10,
                },
            ),
            decision(
                "mean_reversion",
                JudgeState::Disabled,
                JudgeReason::InsufficientSample {
                    trades: 0,
                    required: 10,
                },
            ),
        ];
        let update = update_with(instrument, Some("ema_crossover"), decisions);

        assert!(signal_allowed(
            &update,
            &signal("ema_crossover", instrument, SignalDirection::Long)
        ));
        assert!(signal_allowed(
            &update,
            &signal("mean_reversion", instrument, SignalDirection::Long)
        ));
    }

    /// Uma estratégia comprovadamente inviável (Disabled por um motivo
    /// real, não falta de amostra) continua bloqueada mesmo sem nenhuma
    /// vencedora definida ainda.
    #[test]
    fn a_confirmed_disabled_strategy_stays_blocked_even_without_a_winner() {
        let instrument = InstrumentId::new();
        let decisions = vec![decision(
            "robot-a",
            JudgeState::Disabled,
            JudgeReason::NonPositiveExpectancy {
                expectancy: dec!(-5),
            },
        )];
        let update = update_with(instrument, None, decisions);

        assert!(!signal_allowed(
            &update,
            &signal("robot-a", instrument, SignalDirection::Long)
        ));
    }

    /// Requisito 2: troca de estratégia — a liderança muda de A para B
    /// conforme o histórico evolui, e a troca é registrada com
    /// `previous`/`new` corretos.
    #[test]
    fn detects_a_switch_when_the_recommendation_changes() {
        let instrument = InstrumentId::new();
        let base = Utc::now();
        let candidates = vec![
            StrategyId::new("robot-a").unwrap(),
            StrategyId::new("robot-b").unwrap(),
        ];

        // robot-a: 10 vitórias de +10 -> expectancy=10, líder até aqui.
        let mut history = wins("robot-a", instrument, base, 10, dec!(10));
        // robot-b: só 10 vitórias de +1 por enquanto -> expectancy=1.
        history.extend(wins("robot-b", instrument, base, 10, dec!(1)));

        let mut tracker =
            ActiveStrategyTracker::new(StrategyJudge::new(JudgeThresholds::default()));
        let first = tracker.update(
            "session",
            instrument,
            &candidates,
            &history,
            &[],
            base + Duration::hours(10),
            None,
            None,
        );
        assert_eq!(first.selected, Some(StrategyId::new("robot-a").unwrap()));
        assert_eq!(first.switch, None, "primeira avaliação nunca é uma troca");

        // robot-b acumula um histórico muito melhor a partir daqui ->
        // expectancy dispara e ultrapassa a de robot-a.
        history.extend(wins(
            "robot-b",
            instrument,
            base + Duration::hours(11),
            10,
            dec!(100),
        ));
        let second = tracker.update(
            "session",
            instrument,
            &candidates,
            &history,
            &[],
            base + Duration::hours(21),
            None,
            None,
        );

        assert_eq!(second.selected, Some(StrategyId::new("robot-b").unwrap()));
        let switch = second.switch.expect("expected a switch to be detected");
        assert_eq!(switch.instrument_id, instrument);
        assert_eq!(switch.previous, Some(StrategyId::new("robot-a").unwrap()));
        assert_eq!(switch.new, Some(StrategyId::new("robot-b").unwrap()));
        assert_eq!(switch.timestamp, base + Duration::hours(21));
    }

    /// Requisito 3: posição existente durante uma troca — a posição de
    /// A (destituído) entra em `positions_to_close`; se B também tivesse
    /// uma, permaneceria intocada.
    #[test]
    fn flags_the_displaced_strategys_open_position_for_closing() {
        let instrument = InstrumentId::new();
        let base = Utc::now();
        let candidates = vec![
            StrategyId::new("robot-a").unwrap(),
            StrategyId::new("robot-b").unwrap(),
        ];

        let mut history = wins("robot-a", instrument, base, 10, dec!(1));
        history.extend(wins("robot-b", instrument, base, 10, dec!(100)));
        let open = vec![open_position(
            "robot-a",
            instrument,
            base + Duration::hours(10),
        )];

        let mut tracker =
            ActiveStrategyTracker::new(StrategyJudge::new(JudgeThresholds::default()));
        let update = tracker.update(
            "session",
            instrument,
            &candidates,
            &history,
            &open,
            base + Duration::hours(10),
            None,
            None,
        );

        assert_eq!(update.selected, Some(StrategyId::new("robot-b").unwrap()));
        assert_eq!(update.positions_to_close.len(), 1);
        assert_eq!(
            update.positions_to_close[0].strategy_id,
            StrategyId::new("robot-a").unwrap()
        );

        // Se B também tivesse uma posição aberta, ela não deveria ser
        // sinalizada para fechamento (é o dono selecionado).
        let mut open_both = open;
        open_both.push(open_position(
            "robot-b",
            instrument,
            base + Duration::hours(10),
        ));
        let mut tracker2 =
            ActiveStrategyTracker::new(StrategyJudge::new(JudgeThresholds::default()));
        let update2 = tracker2.update(
            "session",
            instrument,
            &candidates,
            &history,
            &open_both,
            base + Duration::hours(10),
            None,
            None,
        );
        assert_eq!(update2.positions_to_close.len(), 1);
        assert_eq!(
            update2.positions_to_close[0].strategy_id,
            StrategyId::new("robot-a").unwrap()
        );
    }

    /// Requisito 4: ausência de look-ahead — a mesma avaliação em
    /// `as_of` não pode depender de trades fechados depois dele, mesmo
    /// presentes no `trade_history` recebido.
    #[test]
    fn update_is_unaffected_by_trades_closed_after_as_of() {
        let instrument = InstrumentId::new();
        let base = Utc::now();
        let candidates = vec![StrategyId::new("robot-a").unwrap()];
        let as_of = base + Duration::hours(9);

        let prefix = wins("robot-a", instrument, base, 10, dec!(10));
        let mut full = prefix.clone();
        // Trades "futuros": se vazassem, mudariam completamente a
        // expectancy (perdas grandes).
        full.extend((0..10).map(|i| {
            closed_position(
                "robot-a",
                instrument,
                dec!(-1000),
                base + Duration::hours(20 + i),
            )
        }));

        let mut tracker_prefix =
            ActiveStrategyTracker::new(StrategyJudge::new(JudgeThresholds::default()));
        let mut tracker_full =
            ActiveStrategyTracker::new(StrategyJudge::new(JudgeThresholds::default()));

        let from_prefix = tracker_prefix.update(
            "session",
            instrument,
            &candidates,
            &prefix,
            &[],
            as_of,
            None,
            None,
        );
        let from_full = tracker_full.update(
            "session",
            instrument,
            &candidates,
            &full,
            &[],
            as_of,
            None,
            None,
        );

        assert_eq!(from_prefix, from_full);
        assert_eq!(
            from_prefix.selected,
            Some(StrategyId::new("robot-a").unwrap())
        );
    }

    /// Requisito 5: nenhuma troca por uma avaliação ruim isolada — a
    /// estratégia ativa sofre uma sequência de perdas curta demais para
    /// confirmar a demoção (histerese do próprio `StrategyJudge`), e a
    /// seleção nunca muda nesse trecho.
    #[test]
    fn does_not_switch_on_a_single_bad_evaluation() {
        let instrument = InstrumentId::new();
        let base = Utc::now();
        let strategy = "robot-a";
        let candidates = vec![StrategyId::new(strategy).unwrap()];

        let thresholds = JudgeThresholds {
            min_sample_size: 1,
            min_win_rate: dec!(0.5),
            min_profit_factor: Decimal::ZERO,
            max_drawdown: dec!(1_000_000),
            min_confirmations: 3,
        };
        let mut tracker = ActiveStrategyTracker::new(StrategyJudge::new(thresholds));

        // W, W, L, L — win_rate cai para 0.5 no 4º trade (ainda >= 0.5,
        // Active); um único L adicional cairia abaixo, mas isolado não
        // deve confirmar a troca (min_confirmations=3).
        let outcomes = [dec!(100), dec!(100), dec!(-10), dec!(-10), dec!(-10)];
        let positions: Vec<Position> = outcomes
            .iter()
            .enumerate()
            .map(|(i, pnl)| {
                closed_position(strategy, instrument, *pnl, base + Duration::hours(i as i64))
            })
            .collect();

        for i in 0..4 {
            let update = tracker.update(
                "session",
                instrument,
                &candidates,
                &positions,
                &[],
                base + Duration::hours(i),
                None,
                None,
            );
            assert_eq!(update.selected, Some(StrategyId::new(strategy).unwrap()));
            assert_eq!(update.switch, None);
        }

        // i=4: win_rate=2/5=0.4 (<0.5) — primeira avaliação ruim, ainda
        // não confirmada (1 de 3). A seleção deve permanecer robot-a.
        let last = tracker.update(
            "session",
            instrument,
            &candidates,
            &positions,
            &[],
            base + Duration::hours(4),
            None,
            None,
        );
        assert_eq!(last.selected, Some(StrategyId::new(strategy).unwrap()));
        assert_eq!(
            last.switch, None,
            "uma avaliação ruim isolada não deve trocar a estratégia"
        );
        assert_eq!(last.decisions[0].state, JudgeState::Active);
    }

    /// Fase A: dois robôs no mesmo instrumento mantêm Judge/seleção
    /// independentes — a troca em A não sobrescreve o estado de B.
    #[test]
    fn two_robots_on_same_instrument_keep_independent_active_strategy() {
        let instrument = InstrumentId::new();
        let base = Utc::now();
        let candidates_a = vec![
            StrategyId::new("a::momentum").unwrap(),
            StrategyId::new("a::mean_reversion").unwrap(),
        ];
        let candidates_b = vec![
            StrategyId::new("b::momentum").unwrap(),
            StrategyId::new("b::mean_reversion").unwrap(),
        ];

        let mut history = wins("a::momentum", instrument, base, 10, dec!(10));
        history.extend(wins("a::mean_reversion", instrument, base, 10, dec!(1)));
        history.extend(wins("b::mean_reversion", instrument, base, 10, dec!(50)));
        history.extend(wins("b::momentum", instrument, base, 10, dec!(1)));

        let mut tracker =
            ActiveStrategyTracker::new(StrategyJudge::new(JudgeThresholds::default()));

        let for_a = tracker.update(
            "robot-a",
            instrument,
            &candidates_a,
            &history,
            &[],
            base + Duration::hours(10),
            None,
            None,
        );
        let for_b = tracker.update(
            "robot-b",
            instrument,
            &candidates_b,
            &history,
            &[],
            base + Duration::hours(10),
            None,
            None,
        );

        assert_eq!(
            for_a.selected,
            Some(StrategyId::new("a::momentum").unwrap()),
            "robot-a deve escolher entre as próprias candidatas"
        );
        assert_eq!(
            for_b.selected,
            Some(StrategyId::new("b::mean_reversion").unwrap()),
            "robot-b deve escolher independentemente no mesmo instrumento"
        );

        // Melhora mean_reversion de A → troca só em A.
        history.extend(wins(
            "a::mean_reversion",
            instrument,
            base + Duration::hours(11),
            10,
            dec!(100),
        ));
        let after_a = tracker.update(
            "robot-a",
            instrument,
            &candidates_a,
            &history,
            &[],
            base + Duration::hours(21),
            None,
            None,
        );
        let after_b = tracker.update(
            "robot-b",
            instrument,
            &candidates_b,
            &history,
            &[],
            base + Duration::hours(21),
            None,
            None,
        );

        assert_eq!(
            after_a.selected,
            Some(StrategyId::new("a::mean_reversion").unwrap())
        );
        assert!(after_a.switch.is_some());
        assert_eq!(
            after_b.selected,
            Some(StrategyId::new("b::mean_reversion").unwrap()),
            "troca em robot-a não pode alterar a seleção de robot-b"
        );
        assert!(after_b.switch.is_none());
    }
}
