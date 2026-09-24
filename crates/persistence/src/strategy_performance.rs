use chrono::{DateTime, Utc};
use domain::StrategyId;
use rust_decimal::Decimal;
use sqlx::PgPool;

use crate::error::PersistenceError;

/// Um consolidado periódico de desempenho de uma estratégia. É de propósito
/// um registro simples da camada de persistência, em vez de reaproveitar
/// diretamente um tipo do crate `analytics`: este crate depende apenas de
/// `domain`, então cabe ao crate `app` mapear
/// `analytics::PerformanceReport` para este formato ao persistir.
#[derive(Debug, Clone)]
pub struct StrategyPerformanceRecord {
    pub strategy_id: StrategyId,
    pub as_of: DateTime<Utc>,
    pub total_trades: i32,
    pub winners: i32,
    pub losers: i32,
    pub win_rate: Decimal,
    pub gross_pnl: Decimal,
    pub net_pnl: Decimal,
    pub average_win: Decimal,
    pub average_loss: Decimal,
    pub profit_factor: Option<Decimal>,
    pub max_drawdown: Decimal,
}

pub async fn insert(
    pool: &PgPool,
    record: &StrategyPerformanceRecord,
) -> Result<(), PersistenceError> {
    sqlx::query(
        r#"
        INSERT INTO strategy_performance
            (strategy_id, as_of, total_trades, winners, losers, win_rate, gross_pnl, net_pnl,
             average_win, average_loss, profit_factor, max_drawdown)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        "#,
    )
    .bind(record.strategy_id.as_str())
    .bind(record.as_of)
    .bind(record.total_trades)
    .bind(record.winners)
    .bind(record.losers)
    .bind(record.win_rate)
    .bind(record.gross_pnl)
    .bind(record.net_pnl)
    .bind(record.average_win)
    .bind(record.average_loss)
    .bind(record.profit_factor)
    .bind(record.max_drawdown)
    .execute(pool)
    .await?;
    Ok(())
}
