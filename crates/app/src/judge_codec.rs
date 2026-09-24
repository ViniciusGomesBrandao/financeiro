//! Serialização JSON das decisões do Judge para persistência — sem
//! duplicar regras de negócio, só traduz tipos internos para o contrato
//! consumido pela API web.

use rust_decimal::Decimal;
use serde_json::{json, Value};
use strategy_judge::{
    CandidateFit, JudgeDecision, JudgeMetrics, JudgeReason, JudgeState, MarketRegime,
    RegimeAssessment, SelectionReason,
};

use crate::strategy_switch::ActiveStrategyUpdate;

/// Payload v2: regime + seleção + decisões econômicas (array legado embutido).
pub fn encode_evaluation_payload(update: &ActiveStrategyUpdate) -> Value {
    json!({
        "version": 2,
        "regime": encode_regime(&update.regime),
        "selection_reason": encode_selection_reason(&update.selection_reason),
        "candidate_fits": update.candidate_fits.iter().map(encode_candidate_fit).collect::<Vec<_>>(),
        "decisions": update.decisions.iter().map(encode_decision).collect::<Vec<_>>(),
    })
}

pub fn encode_switch_reason(update: &ActiveStrategyUpdate) -> Value {
    json!({
        "economic": encode_reason(
            update
                .switch
                .as_ref()
                .map(|s| s.reason)
                .unwrap_or(JudgeReason::MeetsAllThresholds)
        ),
        "selection": encode_selection_reason(&update.selection_reason),
        "regime": encode_regime(&update.regime),
    })
}

pub fn encode_decisions(decisions: &[JudgeDecision]) -> Value {
    Value::Array(decisions.iter().map(encode_decision).collect())
}

pub fn encode_decision(decision: &JudgeDecision) -> Value {
    json!({
        "timestamp": decision.timestamp,
        "strategy_id": decision.strategy_id.as_str(),
        "instrument_id": decision.instrument_id.0,
        "state": encode_state(decision.state),
        "metrics": encode_metrics(decision.metrics),
        "reason": encode_reason(decision.reason),
    })
}

pub fn encode_state(state: JudgeState) -> &'static str {
    match state {
        JudgeState::Active => "active",
        JudgeState::Degraded => "degraded",
        JudgeState::Disabled => "disabled",
    }
}

pub fn encode_metrics(metrics: JudgeMetrics) -> Value {
    json!({
        "trades": metrics.trades,
        "net_pnl": decimal_str(metrics.net_pnl),
        "expectancy": decimal_str(metrics.expectancy),
        "profit_factor": metrics.profit_factor.map(decimal_str),
        "max_drawdown": decimal_str(metrics.max_drawdown),
        "win_rate": decimal_str(metrics.win_rate),
    })
}

pub fn encode_reason(reason: JudgeReason) -> Value {
    match reason {
        JudgeReason::InsufficientSample { trades, required } => json!({
            "kind": "insufficient_sample",
            "trades": trades,
            "required": required,
        }),
        JudgeReason::MeetsAllThresholds => json!({ "kind": "meets_all_thresholds" }),
        JudgeReason::BelowConsistencyThreshold { win_rate, min } => json!({
            "kind": "below_consistency_threshold",
            "win_rate": decimal_str(win_rate),
            "min": decimal_str(min),
        }),
        JudgeReason::BelowProfitFactorThreshold { profit_factor, min } => json!({
            "kind": "below_profit_factor_threshold",
            "profit_factor": decimal_str(profit_factor),
            "min": decimal_str(min),
        }),
        JudgeReason::NonPositiveExpectancy { expectancy } => json!({
            "kind": "non_positive_expectancy",
            "expectancy": decimal_str(expectancy),
        }),
        JudgeReason::ExcessiveDrawdown { drawdown, max } => json!({
            "kind": "excessive_drawdown",
            "drawdown": decimal_str(drawdown),
            "max": decimal_str(max),
        }),
        JudgeReason::TransitionPending {
            proposed,
            confirmations,
            required,
        } => json!({
            "kind": "transition_pending",
            "proposed": encode_state(proposed),
            "confirmations": confirmations,
            "required": required,
        }),
    }
}

fn encode_regime(regime: &RegimeAssessment) -> Value {
    json!({
        "kind": regime.regime.as_str(),
        "strength": regime.strength,
        "rule": regime.rule,
        "summary": regime.summary,
        "evidence": {
            "r_squared": regime.evidence.r_squared,
            "slope": regime.evidence.slope,
            "autocorrelation": regime.evidence.autocorrelation,
            "bandwidth": regime.evidence.bandwidth,
            "prev_bandwidth": regime.evidence.prev_bandwidth,
            "relative_volume": regime.evidence.relative_volume,
            "percent_b": regime.evidence.percent_b,
            "zscore": regime.evidence.zscore,
        }
    })
}

fn encode_selection_reason(reason: &SelectionReason) -> Value {
    match reason {
        SelectionReason::RegimeFit {
            regime,
            strategy_kind,
            fit_score,
            summary,
        } => json!({
            "kind": "regime_fit",
            "regime": regime_str(*regime),
            "strategy_kind": strategy_kind,
            "fit_score": fit_score,
            "summary": summary,
        }),
        SelectionReason::EconomicFallback { summary } => json!({
            "kind": "economic_fallback",
            "summary": summary,
        }),
        SelectionReason::RegimeBootstrap {
            regime,
            strategy_kind,
            fit_score,
            summary,
        } => json!({
            "kind": "regime_bootstrap",
            "regime": regime_str(*regime),
            "strategy_kind": strategy_kind,
            "fit_score": fit_score,
            "summary": summary,
        }),
        SelectionReason::SelectionPending {
            proposed,
            confirmations,
            required,
            summary,
        } => json!({
            "kind": "selection_pending",
            "proposed": proposed.as_ref().map(|s| s.as_str()),
            "confirmations": confirmations,
            "required": required,
            "summary": summary,
        }),
        SelectionReason::NoViableCandidate { summary } => json!({
            "kind": "no_viable_candidate",
            "summary": summary,
        }),
    }
}

fn encode_candidate_fit(fit: &CandidateFit) -> Value {
    json!({
        "strategy_id": fit.strategy_id.as_str(),
        "strategy_kind": fit.strategy_kind,
        "fit_score": fit.fit_score,
        "economic_state": encode_state(fit.economic_state),
    })
}

fn regime_str(regime: MarketRegime) -> &'static str {
    regime.as_str()
}

fn decimal_str(value: Decimal) -> String {
    value.normalize().to_string()
}

/// Extrai o array de decisões econômicas de um payload v1 (array) ou v2 (objeto).
pub fn decisions_array(payload: &Value) -> Option<&Vec<Value>> {
    match payload {
        Value::Array(arr) => Some(arr),
        Value::Object(map) => map.get("decisions").and_then(|d| d.as_array()),
        _ => None,
    }
}
