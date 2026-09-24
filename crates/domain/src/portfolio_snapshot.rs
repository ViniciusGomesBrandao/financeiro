use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Um snapshot pontual do estado do portfólio, adequado para persistência e
/// posterior análise da curva de capital. O crate de portfólio é responsável por
/// produzi-los; este tipo apenas fixa o formato com o qual todos concordam.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortfolioSnapshot {
    pub timestamp: DateTime<Utc>,
    pub cash: Decimal,
    pub equity: Decimal,
    pub realized_pnl: Decimal,
    pub unrealized_pnl: Decimal,
    pub open_positions_count: u32,
    /// Exposição notional total das posições abertas, como fração do patrimônio
    /// (ex. `0.35` == 35% do patrimônio alocado).
    pub exposure_ratio: Decimal,
    /// Retorno acumulado desde o início, como fração (ex. `0.12` == +12%).
    pub return_pct: Decimal,
    /// P&L realizado no dia UTC corrente (mesmo valor que o Risk Engine
    /// usa para o limite de perda diária).
    pub realized_pnl_today: Decimal,
}
