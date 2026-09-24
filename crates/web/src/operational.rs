//! Handlers do dashboard operacional: robôs, catálogo de estratégias e
//! telemetria do Strategy Judge.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use domain::StrategyId;
use persistence::operational_robots::{self, OperationalRobot, RobotStatus};
use persistence::strategy_configs::StrategyConfigRecord;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::Value;

use crate::dto::{
    CreateRobotDto, JudgeEvaluationDto, OperationalRobotDto, PnlCurvePointDto, RobotDetailDto,
    StrategyCatalogEntryDto, StrategyPerformanceDto, StrategySwitchDto, TradeDto,
};
use crate::error::AppError;
use crate::state::AppState;

pub async fn strategy_catalog() -> Result<Json<Vec<StrategyCatalogEntryDto>>, AppError> {
    let dtos = strategies::catalog::entries()
        .into_iter()
        .map(|entry| StrategyCatalogEntryDto {
            kind: entry.descriptor.id.to_string(),
            display_name: entry.descriptor.display_name.to_string(),
            description: entry.descriptor.description.to_string(),
            category: match entry.descriptor.category {
                strategies::catalog::StrategyCategory::Baseline => "baseline".to_string(),
                strategies::catalog::StrategyCategory::Quantitative => "quantitative".to_string(),
            },
        })
        .collect();
    Ok(Json(dtos))
}

pub async fn list_robots(
    State(state): State<AppState>,
) -> Result<Json<Vec<OperationalRobotDto>>, AppError> {
    let robots = operational_robots::list_all(&state.pool).await?;
    let active_states = persistence::active_strategy_state::list_active_states(&state.pool).await?;
    let closed = persistence::positions::list_closed(&state.pool).await?;
    let open = persistence::positions::list_open(&state.pool).await?;
    let evaluations = persistence::judge_telemetry::latest_evaluations(&state.pool, 200).await?;
    let snapshots = persistence::robot_portfolio_snapshots::latest_all(&state.pool).await?;
    let prices = persistence::latest_prices::list_all(&state.pool).await?;
    let price_by_instrument: HashMap<_, _> = prices
        .into_iter()
        .map(|p| (p.instrument_id, p.price))
        .collect();
    let snap_by_robot: HashMap<_, _> = snapshots
        .into_iter()
        .map(|s| (s.robot_id, s.snapshot))
        .collect();

    let mut dtos = Vec::with_capacity(robots.len());
    for robot in robots {
        let latest = latest_eval_for_robot(&robot.id, &evaluations);
        dtos.push(enrich_robot(
            &robot,
            &active_states,
            &closed,
            &open,
            latest.as_ref().map(|e| &e.decisions_json),
            snap_by_robot.get(&robot.id),
            &price_by_instrument,
        ));
    }
    Ok(Json(dtos))
}

pub async fn get_robot_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RobotDetailDto>, AppError> {
    let robot = operational_robots::get(&state.pool, &id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("robô {id} não encontrado")))?;

    let symbols = crate::handlers::symbol_lookup(&state.pool).await?;
    let active_states = persistence::active_strategy_state::list_active_states(&state.pool).await?;
    let closed = persistence::positions::list_closed(&state.pool).await?;
    let open = persistence::positions::list_open(&state.pool).await?;
    let evaluations =
        persistence::judge_telemetry::list_evaluations_for_robot(&state.pool, &id, 40).await?;
    let switches =
        persistence::judge_telemetry::list_switches_for_robot(&state.pool, &id, 40).await?;
    let all_trades = persistence::trades::list_all(&state.pool).await?;
    let snapshot =
        persistence::robot_portfolio_snapshots::latest_for_robot(&state.pool, &id).await?;
    let prices = persistence::latest_prices::list_all(&state.pool).await?;
    let price_by_instrument: HashMap<_, _> = prices
        .into_iter()
        .map(|p| (p.instrument_id, p.price))
        .collect();

    let instance_ids: Vec<String> = robot
        .candidate_kinds
        .iter()
        .map(|kind| operational_robots::strategy_instance_id(&robot.id, kind))
        .collect();

    let latest_json = evaluations.first().map(|e| &e.decisions_json);
    let robot_dto = enrich_robot(
        &robot,
        &active_states,
        &closed,
        &open,
        latest_json,
        snapshot.as_ref(),
        &price_by_instrument,
    );

    let mut trades: Vec<TradeDto> = all_trades
        .into_iter()
        .filter(|t| instance_ids.iter().any(|i| i == t.strategy_id.as_str()))
        .take(100)
        .map(|trade| TradeDto {
            symbol: symbols
                .get(&trade.instrument_id)
                .cloned()
                .unwrap_or_else(|| "?".to_string()),
            strategy_id: trade.strategy_id.as_str().to_string(),
            side: format!("{:?}", trade.side),
            quantity: trade.quantity,
            entry_price: trade.entry_price,
            exit_price: trade.exit_price,
            opened_at: trade.opened_at,
            closed_at: trade.closed_at,
            pnl_gross: trade.pnl_gross,
            fees_paid: trade.fees_paid,
            spread_paid: trade.spread_paid,
            slippage_paid: trade.slippage_paid,
            pnl_net: trade.pnl_net,
        })
        .collect();
    // list_all já vem DESC; curva precisa ASC
    trades.sort_by_key(|a| a.closed_at);
    let realized_pnl_curve = cumulative_pnl_curve(&trades);
    trades.reverse();

    let mut candidate_performance: Vec<StrategyPerformanceDto> = instance_ids
        .iter()
        .map(|sid| {
            let positions: Vec<_> = closed
                .iter()
                .filter(|p| p.strategy_id.as_str() == sid)
                .cloned()
                .collect();
            let report = analytics::compute_performance(&positions);
            StrategyPerformanceDto {
                strategy_id: sid.clone(),
                total_trades: report.total_trades,
                winners: report.winners,
                losers: report.losers,
                win_rate: report.win_rate,
                gross_pnl: report.gross_pnl,
                net_pnl: report.net_pnl,
                average_win: report.average_win,
                average_loss: report.average_loss,
                profit_factor: report.profit_factor,
                max_drawdown: report.max_drawdown,
            }
        })
        .collect();
    candidate_performance.sort_by_key(|b| std::cmp::Reverse(b.net_pnl));

    let eval_dtos: Vec<JudgeEvaluationDto> = evaluations
        .into_iter()
        .map(|e| JudgeEvaluationDto {
            id: e.id,
            symbol: symbols
                .get(&e.instrument_id)
                .cloned()
                .unwrap_or_else(|| robot.symbol.clone()),
            robot_id: e.robot_id,
            evaluated_at: e.evaluated_at,
            selected_strategy_id: e.selected_strategy_id,
            decisions: e.decisions_json,
        })
        .collect();

    let switch_dtos: Vec<StrategySwitchDto> = switches
        .into_iter()
        .map(|s| StrategySwitchDto {
            id: s.id,
            symbol: symbols
                .get(&s.instrument_id)
                .cloned()
                .unwrap_or_else(|| robot.symbol.clone()),
            robot_id: s.robot_id,
            previous_strategy_id: s.previous_strategy_id,
            new_strategy_id: s.new_strategy_id,
            reason: s.reason_json,
            switched_at: s.switched_at,
        })
        .collect();

    Ok(Json(RobotDetailDto {
        robot: robot_dto,
        trades,
        evaluations: eval_dtos,
        switches: switch_dtos,
        realized_pnl_curve,
        candidate_performance,
    }))
}

pub async fn create_robot(
    State(state): State<AppState>,
    Json(body): Json<CreateRobotDto>,
) -> Result<(StatusCode, Json<OperationalRobotDto>), AppError> {
    validate_robot_input(&body)?;

    if operational_robots::get(&state.pool, &body.id)
        .await?
        .is_some()
    {
        return Err(AppError::bad_request(format!(
            "já existe um robô com id {:?}",
            body.id
        )));
    }

    let now = Utc::now();
    let robot = OperationalRobot {
        id: body.id.clone(),
        name: body.name.clone(),
        symbol: body.symbol.clone(),
        timeframe: body.timeframe.clone(),
        candidate_kinds: body.candidate_kinds.clone(),
        paper_capital: body.paper_capital,
        status: RobotStatus::Stopped,
        created_at: now,
        updated_at: now,
    };
    operational_robots::insert(&state.pool, &robot).await?;

    for kind in &robot.candidate_kinds {
        let descriptor = strategies::catalog::get(kind)
            .ok_or_else(|| AppError::bad_request(format!("estratégia desconhecida: {kind}")))?;
        let instance_id =
            StrategyId::new(operational_robots::strategy_instance_id(&robot.id, kind))
                .map_err(|e| AppError::bad_request(e.to_string()))?;
        let record = StrategyConfigRecord {
            id: instance_id,
            strategy_kind: kind.clone(),
            params: descriptor.descriptor.default_params,
            supported_asset_classes: descriptor
                .descriptor
                .requirements
                .supported_asset_classes
                .clone(),
            required_market_data: descriptor
                .descriptor
                .requirements
                .required_market_data
                .clone(),
            enabled: false,
        };
        persistence::strategy_configs::upsert(&state.pool, &record).await?;
    }

    Ok((
        StatusCode::CREATED,
        Json(enrich_robot(
            &robot,
            &[],
            &[],
            &[],
            None,
            None,
            &HashMap::new(),
        )),
    ))
}

#[derive(Debug, Deserialize)]
pub struct RobotStatusBody {
    pub status: String,
}

pub async fn set_robot_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RobotStatusBody>,
) -> Result<Json<OperationalRobotDto>, AppError> {
    let status = match body.status.as_str() {
        "running" => RobotStatus::Running,
        "stopped" => RobotStatus::Stopped,
        other => {
            return Err(AppError::bad_request(format!(
                "status inválido {other:?}, use running ou stopped"
            )))
        }
    };

    let robot = operational_robots::get(&state.pool, &id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("robô {id} não encontrado")))?;

    operational_robots::set_status(&state.pool, &id, status).await?;

    let enabled = status == RobotStatus::Running;
    for kind in &robot.candidate_kinds {
        let instance_id = StrategyId::new(operational_robots::strategy_instance_id(&id, kind))
            .map_err(|e| AppError::bad_request(e.to_string()))?;
        persistence::strategy_configs::set_enabled(&state.pool, &instance_id, enabled).await?;
    }

    let mut updated = robot;
    updated.status = status;
    updated.updated_at = Utc::now();

    let active_states = persistence::active_strategy_state::list_active_states(&state.pool).await?;
    let closed = persistence::positions::list_closed(&state.pool).await?;
    let open = persistence::positions::list_open(&state.pool).await?;
    let evaluations =
        persistence::judge_telemetry::list_evaluations_for_robot(&state.pool, &id, 1).await?;
    let snapshot =
        persistence::robot_portfolio_snapshots::latest_for_robot(&state.pool, &id).await?;
    let prices = persistence::latest_prices::list_all(&state.pool).await?;
    let price_by_instrument: HashMap<_, _> = prices
        .into_iter()
        .map(|p| (p.instrument_id, p.price))
        .collect();

    Ok(Json(enrich_robot(
        &updated,
        &active_states,
        &closed,
        &open,
        evaluations.first().map(|e| &e.decisions_json),
        snapshot.as_ref(),
        &price_by_instrument,
    )))
}

pub async fn judge_evaluations(
    State(state): State<AppState>,
) -> Result<Json<Vec<JudgeEvaluationDto>>, AppError> {
    let symbols = crate::handlers::symbol_lookup(&state.pool).await?;
    let evaluations = persistence::judge_telemetry::latest_evaluations(&state.pool, 50).await?;

    let dtos = evaluations
        .into_iter()
        .map(|e| JudgeEvaluationDto {
            id: e.id,
            symbol: symbols
                .get(&e.instrument_id)
                .cloned()
                .unwrap_or_else(|| "?".to_string()),
            robot_id: e.robot_id,
            evaluated_at: e.evaluated_at,
            selected_strategy_id: e.selected_strategy_id,
            decisions: e.decisions_json,
        })
        .collect();
    Ok(Json(dtos))
}

pub async fn strategy_switches(
    State(state): State<AppState>,
) -> Result<Json<Vec<StrategySwitchDto>>, AppError> {
    let symbols = crate::handlers::symbol_lookup(&state.pool).await?;
    let switches = persistence::judge_telemetry::list_switches(&state.pool, 50).await?;

    let dtos = switches
        .into_iter()
        .map(|s| StrategySwitchDto {
            id: s.id,
            symbol: symbols
                .get(&s.instrument_id)
                .cloned()
                .unwrap_or_else(|| "?".to_string()),
            robot_id: s.robot_id,
            previous_strategy_id: s.previous_strategy_id,
            new_strategy_id: s.new_strategy_id,
            reason: s.reason_json,
            switched_at: s.switched_at,
        })
        .collect();
    Ok(Json(dtos))
}

fn validate_robot_input(body: &CreateRobotDto) -> Result<(), AppError> {
    if body.id.trim().is_empty() {
        return Err(AppError::bad_request("id é obrigatório"));
    }
    if body.name.trim().is_empty() {
        return Err(AppError::bad_request("nome é obrigatório"));
    }
    if body.symbol.split_once('/').is_none() {
        return Err(AppError::bad_request("símbolo deve ser BASE/QUOTE"));
    }
    if domain::Timeframe::parse(&body.timeframe).is_none() {
        return Err(AppError::bad_request(
            "timeframe inválido; use 1m, 5m, 15m, 30m, 1h, 4h, 1d ou 1w",
        ));
    }
    if body.candidate_kinds.is_empty() {
        return Err(AppError::bad_request(
            "escolha ao menos uma estratégia candidata",
        ));
    }
    if body.paper_capital <= Decimal::ZERO {
        return Err(AppError::bad_request("capital fictício deve ser positivo"));
    }
    for kind in &body.candidate_kinds {
        if strategies::catalog::get(kind).is_none() {
            return Err(AppError::bad_request(format!(
                "estratégia desconhecida: {kind}"
            )));
        }
    }
    Ok(())
}

fn latest_eval_for_robot(
    robot_id: &str,
    evaluations: &[persistence::judge_telemetry::JudgeEvaluationRecord],
) -> Option<persistence::judge_telemetry::JudgeEvaluationRecord> {
    evaluations
        .iter()
        .find(|e| e.robot_id.as_deref() == Some(robot_id))
        .cloned()
}

fn enrich_robot(
    robot: &OperationalRobot,
    active_states: &[persistence::active_strategy_state::ActiveStrategyState],
    closed: &[domain::Position],
    open: &[domain::Position],
    decisions_json: Option<&Value>,
    snapshot: Option<&domain::PortfolioSnapshot>,
    price_by_instrument: &HashMap<domain::InstrumentId, Decimal>,
) -> OperationalRobotDto {
    let instance_ids: Vec<String> = robot
        .candidate_kinds
        .iter()
        .map(|kind| operational_robots::strategy_instance_id(&robot.id, kind))
        .collect();

    let active = active_states
        .iter()
        .find(|s| s.robot_id == robot.id)
        .cloned();

    let active_strategy_id = active
        .as_ref()
        .and_then(|s| s.selected_strategy_id.clone())
        .filter(|sid| instance_ids.iter().any(|i| i == sid));

    let relevant_closed: Vec<_> = closed
        .iter()
        .filter(|p| instance_ids.iter().any(|i| i == p.strategy_id.as_str()))
        .cloned()
        .collect();
    let relevant_open: Vec<_> = open
        .iter()
        .filter(|p| instance_ids.iter().any(|i| i == p.strategy_id.as_str()))
        .cloned()
        .collect();

    let performance = analytics::compute_performance(&relevant_closed);
    let context = judge_context(active_strategy_id.as_deref(), decisions_json);

    // Equity isolada: preferir snapshot do robô; se ausente, capital inicial
    // + unrealized das posições deste robô (ainda sem candle processado).
    let unrealized_now: Decimal = relevant_open
        .iter()
        .map(|p| {
            let mark = price_by_instrument
                .get(&p.instrument_id)
                .copied()
                .unwrap_or(p.entry_price);
            p.unrealized_pnl(mark)
        })
        .sum();

    let (cash, equity, unrealized_pnl, return_pct) = if let Some(snap) = snapshot {
        (
            Some(snap.cash),
            Some(snap.equity),
            Some(snap.unrealized_pnl),
            Some(snap.return_pct),
        )
    } else {
        let cash = robot.paper_capital;
        let equity = cash + unrealized_now;
        let ret = if robot.paper_capital > Decimal::ZERO {
            (equity - robot.paper_capital) / robot.paper_capital
        } else {
            Decimal::ZERO
        };
        (Some(cash), Some(equity), Some(unrealized_now), Some(ret))
    };

    OperationalRobotDto {
        id: robot.id.clone(),
        name: robot.name.clone(),
        symbol: robot.symbol.clone(),
        timeframe: robot.timeframe.clone(),
        candidate_kinds: robot.candidate_kinds.clone(),
        strategy_instance_ids: instance_ids,
        paper_capital: robot.paper_capital,
        status: robot.status.as_str().to_string(),
        active_strategy_id,
        judge_evaluated_at: active.as_ref().map(|s| s.evaluated_at),
        judge_mood: context.mood,
        active_why: context.why,
        market_regime: context.market_regime,
        regime_strength: context.regime_strength,
        regime_summary: context.regime_summary,
        selection_fit_score: context.selection_fit_score,
        candidate_fits: context.candidate_fits,
        cash,
        equity,
        unrealized_pnl,
        return_pct,
        open_positions_count: relevant_open.len(),
        closed_trades_count: performance.total_trades,
        net_pnl: performance.net_pnl,
        win_rate: performance.win_rate,
        profit_factor: performance.profit_factor,
        expectancy: performance.expectancy,
        max_drawdown: performance.max_drawdown,
        created_at: robot.created_at,
        updated_at: robot.updated_at,
        engine_restart_required: robot.status == RobotStatus::Running,
    }
}

struct JudgeContextView {
    mood: String,
    why: String,
    market_regime: Option<String>,
    regime_strength: Option<f64>,
    regime_summary: Option<String>,
    selection_fit_score: Option<f64>,
    candidate_fits: Option<Value>,
}

fn decisions_from_payload(payload: &Value) -> Option<&Vec<Value>> {
    match payload {
        Value::Array(arr) => Some(arr),
        Value::Object(map) => map.get("decisions").and_then(|d| d.as_array()),
        _ => None,
    }
}

/// Lê o JSON já persistido pelo pipeline — não recalcula o Judge.
fn judge_context(
    active_strategy_id: Option<&str>,
    decisions_json: Option<&Value>,
) -> JudgeContextView {
    let idle = JudgeContextView {
        mood: "idle".to_string(),
        why: "O Judge ainda não avaliou este mercado para este robô.".to_string(),
        market_regime: None,
        regime_strength: None,
        regime_summary: None,
        selection_fit_score: None,
        candidate_fits: None,
    };

    let Some(payload) = decisions_json else {
        return idle;
    };

    let decisions = decisions_from_payload(payload);
    let regime = payload.get("regime");
    let selection = payload.get("selection_reason");
    let market_regime = regime
        .and_then(|r| r.get("kind"))
        .and_then(|k| k.as_str())
        .map(str::to_string);
    let regime_strength = regime
        .and_then(|r| r.get("strength"))
        .and_then(|v| v.as_f64());
    let regime_summary = regime
        .and_then(|r| r.get("summary"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let selection_fit_score = selection
        .and_then(|s| s.get("fit_score"))
        .and_then(|v| v.as_f64());
    let candidate_fits = payload.get("candidate_fits").cloned();

    let selection_summary = selection
        .and_then(|s| s.get("summary"))
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let Some(decisions) = decisions else {
        return idle;
    };

    if let Some(active_id) = active_strategy_id {
        let decision = decisions.iter().find(|d| {
            d.get("strategy_id")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s == active_id)
        });
        let why = selection_summary.unwrap_or_else(|| {
            decision
                .and_then(|d| d.get("reason"))
                .map(reason_kind_sentence)
                .unwrap_or_else(|| {
                    "É a candidata que o Judge considera mais adequada agora.".to_string()
                })
        });
        let state = decision
            .and_then(|d| d.get("state"))
            .and_then(|v| v.as_str())
            .unwrap_or("active");
        let mood = if state == "active"
            || selection
                .and_then(|s| s.get("kind"))
                .and_then(|k| k.as_str())
                .is_some_and(|k| k == "regime_fit" || k == "regime_bootstrap")
        {
            "satisfied"
        } else {
            "looking"
        };
        return JudgeContextView {
            mood: mood.to_string(),
            why,
            market_regime,
            regime_strength,
            regime_summary,
            selection_fit_score,
            candidate_fits,
        };
    }

    if let Some(summary) = selection_summary {
        return JudgeContextView {
            mood: "looking".to_string(),
            why: summary,
            market_regime,
            regime_strength,
            regime_summary,
            selection_fit_score,
            candidate_fits,
        };
    }

    let any_transition = decisions.iter().any(|d| {
        d.get("reason")
            .and_then(|r| r.get("kind"))
            .and_then(|k| k.as_str())
            == Some("transition_pending")
    });
    let all_insufficient = !decisions.is_empty()
        && decisions.iter().all(|d| {
            d.get("reason")
                .and_then(|r| r.get("kind"))
                .and_then(|k| k.as_str())
                == Some("insufficient_sample")
        });

    let why = if all_insufficient {
        "Ainda faltam trades fechados para o Judge escolher com segurança.".to_string()
    } else if any_transition {
        "O Judge está confirmando uma mudança — ainda não trocou.".to_string()
    } else {
        "Nenhuma candidata passou nos critérios agora; o Judge continua avaliando.".to_string()
    };

    JudgeContextView {
        mood: "looking".to_string(),
        why,
        market_regime,
        regime_strength,
        regime_summary,
        selection_fit_score,
        candidate_fits,
    }
}

fn reason_kind_sentence(reason: &Value) -> String {
    match reason.get("kind").and_then(|k| k.as_str()) {
        Some("meets_all_thresholds") => {
            "Passou em todos os critérios econômicos — o Judge está satisfeito.".to_string()
        }
        Some("insufficient_sample") => {
            "Ainda há poucos trades fechados para uma opinião firme.".to_string()
        }
        Some("below_consistency_threshold") => {
            "Taxa de acerto abaixo do mínimo, mas ainda é a melhor opção disponível.".to_string()
        }
        Some("below_profit_factor_threshold") => {
            "Profit factor abaixo do mínimo, mas ainda lidera entre as candidatas.".to_string()
        }
        Some("non_positive_expectancy") => {
            "Expectancy fraca — o Judge pode procurar outra em breve.".to_string()
        }
        Some("excessive_drawdown") => "Drawdown alto — o Judge acompanha com atenção.".to_string(),
        Some("transition_pending") => {
            "Há uma mudança sugerida, aguardando confirmações antes de trocar.".to_string()
        }
        _ => "É a candidata que o Judge considera mais adequada agora.".to_string(),
    }
}

fn cumulative_pnl_curve(trades_asc: &[TradeDto]) -> Vec<PnlCurvePointDto> {
    let mut cumulative = Decimal::ZERO;
    trades_asc
        .iter()
        .map(|t| {
            cumulative += t.pnl_net;
            PnlCurvePointDto {
                at: t.closed_at,
                cumulative_pnl: cumulative,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn judge_mood_satisfied_when_active_meets_thresholds() {
        let decisions = json!([{
            "strategy_id": "bot::momentum",
            "state": "active",
            "reason": { "kind": "meets_all_thresholds" }
        }]);
        let ctx = judge_context(Some("bot::momentum"), Some(&decisions));
        assert_eq!(ctx.mood, "satisfied");
        assert!(ctx.why.contains("satisfeito") || ctx.why.contains("critérios"));
    }

    #[test]
    fn judge_mood_looking_without_selection() {
        let decisions = json!([{
            "strategy_id": "bot::momentum",
            "state": "disabled",
            "reason": { "kind": "insufficient_sample", "trades": 0, "required": 10 }
        }]);
        let ctx = judge_context(None, Some(&decisions));
        assert_eq!(ctx.mood, "looking");
        assert!(ctx.why.contains("trades") || ctx.why.contains("faltam"));
    }

    #[test]
    fn judge_context_reads_regime_from_v2_payload() {
        let payload = json!({
            "version": 2,
            "regime": {
                "kind": "trending",
                "strength": 0.8,
                "rule": "trend_r2_and_autocorr",
                "summary": "tendência de alta com R²=0.85"
            },
            "selection_reason": {
                "kind": "regime_bootstrap",
                "regime": "trending",
                "strategy_kind": "momentum",
                "fit_score": 1.0,
                "summary": "momentum selecionada no bootstrap (regime trending)"
            },
            "candidate_fits": [],
            "decisions": [{
                "strategy_id": "bot::momentum",
                "state": "disabled",
                "reason": { "kind": "insufficient_sample", "trades": 0, "required": 10 }
            }]
        });
        let ctx = judge_context(Some("bot::momentum"), Some(&payload));
        assert_eq!(ctx.mood, "satisfied");
        assert_eq!(ctx.market_regime.as_deref(), Some("trending"));
        assert!(ctx.why.contains("momentum"));
        assert_eq!(ctx.selection_fit_score, Some(1.0));
    }

    #[test]
    fn cumulative_pnl_curve_sums_in_order() {
        use chrono::TimeZone;
        let t0 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let t1 = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let trades = vec![
            TradeDto {
                symbol: "BTC/USDT".into(),
                strategy_id: "a".into(),
                side: "Buy".into(),
                quantity: Decimal::ONE,
                entry_price: Decimal::ONE,
                exit_price: Decimal::ONE,
                opened_at: t0,
                closed_at: t0,
                pnl_gross: Decimal::new(10, 0),
                fees_paid: Decimal::ZERO,
                spread_paid: Decimal::ZERO,
                slippage_paid: Decimal::ZERO,
                pnl_net: Decimal::new(10, 0),
            },
            TradeDto {
                symbol: "BTC/USDT".into(),
                strategy_id: "a".into(),
                side: "Buy".into(),
                quantity: Decimal::ONE,
                entry_price: Decimal::ONE,
                exit_price: Decimal::ONE,
                opened_at: t1,
                closed_at: t1,
                pnl_gross: Decimal::new(-3, 0),
                fees_paid: Decimal::ZERO,
                spread_paid: Decimal::ZERO,
                slippage_paid: Decimal::ZERO,
                pnl_net: Decimal::new(-3, 0),
            },
        ];
        let curve = cumulative_pnl_curve(&trades);
        assert_eq!(curve.len(), 2);
        assert_eq!(curve[0].cumulative_pnl, Decimal::new(10, 0));
        assert_eq!(curve[1].cumulative_pnl, Decimal::new(7, 0));
    }
}
