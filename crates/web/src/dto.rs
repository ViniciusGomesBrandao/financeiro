//! Formatos JSON servidos pela API. Deliberadamente separados dos tipos de
//! `domain`/`persistence`: este crate só lê e apresenta — nenhum campo
//! aqui é calculado a partir de regra financeira nova, só reaproveita o
//! que `domain`/`portfolio`/`analytics` já calcularam (ver os handlers em
//! `handlers.rs`).

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct OverviewDto {
    pub as_of: Option<DateTime<Utc>>,
    pub cash: Option<Decimal>,
    pub equity: Option<Decimal>,
    pub realized_pnl_total: Option<Decimal>,
    pub realized_pnl_today: Option<Decimal>,
    pub unrealized_pnl: Option<Decimal>,
    pub return_pct: Option<Decimal>,
    pub max_drawdown: Decimal,
    pub open_positions_count: Option<u32>,
    pub exposure_ratio: Option<Decimal>,
}

#[derive(Debug, Serialize)]
pub struct PriceDto {
    pub symbol: String,
    pub price: Decimal,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct OpenPositionDto {
    pub symbol: String,
    pub strategy_id: String,
    pub side: String,
    pub quantity: Decimal,
    pub entry_price: Decimal,
    pub current_price: Option<Decimal>,
    pub unrealized_pnl: Option<Decimal>,
    pub opened_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct TradeDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    pub symbol: String,
    pub strategy_id: String,
    pub side: String,
    pub quantity: Decimal,
    pub entry_price: Decimal,
    pub exit_price: Decimal,
    pub opened_at: DateTime<Utc>,
    pub closed_at: DateTime<Utc>,
    pub pnl_gross: Decimal,
    pub fees_paid: Decimal,
    pub spread_paid: Decimal,
    pub slippage_paid: Decimal,
    pub pnl_net: Decimal,
    /// Motivo da saída, quando correlacionável a `risk_decisions` (já persistido).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_trigger: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_direction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_confidence: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct StrategyPerformanceDto {
    pub strategy_id: String,
    pub total_trades: usize,
    pub winners: usize,
    pub losers: usize,
    pub win_rate: Decimal,
    pub gross_pnl: Decimal,
    pub net_pnl: Decimal,
    pub average_win: Decimal,
    pub average_loss: Decimal,
    pub profit_factor: Option<Decimal>,
    pub max_drawdown: Decimal,
}

#[derive(Debug, Serialize)]
pub struct TimelineEntryDto {
    pub created_at: DateTime<Utc>,
    pub symbol: String,
    pub strategy_id: String,
    pub trigger: String,
    pub approved: bool,
    pub reason: Option<String>,
    pub signal_direction: Option<String>,
    pub signal_confidence: Option<f64>,
    pub order_side: Option<String>,
    pub order_quantity: Option<Decimal>,
    pub fill_price: Option<Decimal>,
    pub fee: Option<Decimal>,
    pub spread_cost: Option<Decimal>,
    pub slippage_cost: Option<Decimal>,
    pub pnl_gross: Option<Decimal>,
    pub pnl_net: Option<Decimal>,
}

impl From<persistence::risk_decisions::TimelineEntry> for TimelineEntryDto {
    fn from(entry: persistence::risk_decisions::TimelineEntry) -> Self {
        Self {
            created_at: entry.created_at,
            symbol: entry.symbol,
            strategy_id: entry.strategy_id,
            trigger: entry.trigger,
            approved: entry.approved,
            reason: entry.reason,
            signal_direction: entry.signal_direction,
            signal_confidence: entry.signal_confidence,
            order_side: entry.order_side,
            order_quantity: entry.order_quantity,
            fill_price: entry.fill_price,
            fee: entry.fill_fee,
            spread_cost: entry.fill_spread_cost,
            slippage_cost: entry.fill_slippage_cost,
            pnl_gross: entry.pnl_gross,
            pnl_net: entry.pnl_net,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct StrategyCatalogEntryDto {
    pub kind: String,
    pub display_name: String,
    pub description: String,
    pub category: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateRobotDto {
    pub id: String,
    pub name: String,
    pub symbol: String,
    pub timeframe: String,
    pub candidate_kinds: Vec<String>,
    pub paper_capital: Decimal,
}

#[derive(Debug, Serialize)]
pub struct OperationalRobotDto {
    pub id: String,
    pub name: String,
    pub symbol: String,
    pub timeframe: String,
    pub candidate_kinds: Vec<String>,
    pub strategy_instance_ids: Vec<String>,
    pub paper_capital: Decimal,
    pub status: String,
    pub active_strategy_id: Option<String>,
    pub judge_evaluated_at: Option<DateTime<Utc>>,
    /// `satisfied` | `looking` | `idle` — leitura leiga do humor do Judge.
    pub judge_mood: String,
    /// Frase curta: por que a estratégia ativa (ou a ausência dela).
    pub active_why: String,
    /// Regime de mercado classificado pelo Judge (`trending`, `ranging`, …).
    pub market_regime: Option<String>,
    /// Força heurística da classificação de regime em `[0, 1]` (não probabilidade).
    pub regime_strength: Option<f64>,
    /// Resumo curto do regime (evidência).
    pub regime_summary: Option<String>,
    /// Score de afinidade da estratégia selecionada ao regime (heurístico).
    pub selection_fit_score: Option<f64>,
    /// Afinidade de cada candidata ao regime atual (JSON).
    pub candidate_fits: Option<Value>,
    /// Caixa isolado do robô (último snapshot); `None` se ainda não operou.
    pub cash: Option<Decimal>,
    /// Equity isolada: cash + mark das posições abertas deste robô.
    pub equity: Option<Decimal>,
    pub unrealized_pnl: Option<Decimal>,
    /// Retorno % sobre o capital inicial (`paper_capital`) deste robô.
    pub return_pct: Option<Decimal>,
    pub open_positions_count: usize,
    pub closed_trades_count: usize,
    pub net_pnl: Decimal,
    pub win_rate: Decimal,
    pub profit_factor: Option<Decimal>,
    pub expectancy: Decimal,
    pub max_drawdown: Decimal,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Novas instâncias só entram no registry no próximo boot do quant-engine.
    pub engine_restart_required: bool,
}

#[derive(Debug, Serialize)]
pub struct RobotDetailDto {
    pub robot: OperationalRobotDto,
    pub trades: Vec<TradeDto>,
    pub evaluations: Vec<JudgeEvaluationDto>,
    pub switches: Vec<StrategySwitchDto>,
    /// P&L líquido acumulado dos trades deste robô (não é equity da conta global).
    pub realized_pnl_curve: Vec<PnlCurvePointDto>,
    /// Equity isolada do robô ao longo do tempo (snapshots já gravados).
    pub equity_curve: Vec<EquityCurvePointDto>,
    pub candidate_performance: Vec<StrategyPerformanceDto>,
}

#[derive(Debug, Serialize)]
pub struct EquityCurvePointDto {
    pub at: DateTime<Utc>,
    pub equity: Decimal,
    pub cash: Decimal,
    pub return_pct: Decimal,
}

#[derive(Debug, Serialize)]
pub struct PnlCurvePointDto {
    pub at: DateTime<Utc>,
    pub cumulative_pnl: Decimal,
}

#[derive(Debug, Serialize)]
pub struct JudgeEvaluationDto {
    pub id: Uuid,
    pub symbol: String,
    pub robot_id: Option<String>,
    pub evaluated_at: DateTime<Utc>,
    pub selected_strategy_id: Option<String>,
    pub decisions: Value,
}

#[derive(Debug, Serialize)]
pub struct StrategySwitchDto {
    pub id: Uuid,
    pub symbol: String,
    pub robot_id: Option<String>,
    pub previous_strategy_id: Option<String>,
    pub new_strategy_id: Option<String>,
    pub reason: Value,
    pub switched_at: DateTime<Utc>,
}
