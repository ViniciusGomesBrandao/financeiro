use chrono::{DateTime, Utc};
use domain::{InstrumentId, Position, StrategyId};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::codec::{position_status_from_str, position_status_to_str, side_from_str, side_to_str};
use crate::error::PersistenceError;

#[derive(sqlx::FromRow)]
struct PositionRow {
    id: Uuid,
    instrument_id: Uuid,
    strategy_id: String,
    side: String,
    quantity: Decimal,
    entry_price: Decimal,
    exit_price: Option<Decimal>,
    opened_at: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
    status: String,
    realized_pnl_gross: Option<Decimal>,
    realized_pnl_net: Option<Decimal>,
    fees_paid: Decimal,
    spread_paid: Decimal,
    slippage_paid: Decimal,
}

fn row_to_position(row: PositionRow) -> Result<Position, PersistenceError> {
    Ok(Position {
        id: row.id,
        instrument_id: InstrumentId(row.instrument_id),
        strategy_id: StrategyId::new(row.strategy_id)
            .map_err(|e| PersistenceError::Database(sqlx::Error::Decode(e.to_string().into())))?,
        side: side_from_str(&row.side)?,
        quantity: row.quantity,
        entry_price: row.entry_price,
        exit_price: row.exit_price,
        opened_at: row.opened_at,
        closed_at: row.closed_at,
        status: position_status_from_str(&row.status)?,
        realized_pnl_gross: row.realized_pnl_gross,
        realized_pnl_net: row.realized_pnl_net,
        fees_paid: row.fees_paid,
        spread_paid: row.spread_paid,
        slippage_paid: row.slippage_paid,
    })
}

/// Insere ou substitui integralmente a linha de `position.id`. Uma posição
/// passa por exatamente dois estados (open -> closed), cada um representado
/// por um upsert da linha completa — não há contabilidade de fill parcial
/// para reconciliar.
pub async fn upsert(pool: &PgPool, position: &Position) -> Result<(), PersistenceError> {
    sqlx::query(
        r#"
        INSERT INTO positions
            (id, instrument_id, strategy_id, side, quantity, entry_price, exit_price,
             opened_at, closed_at, status, realized_pnl_gross, realized_pnl_net, fees_paid,
             spread_paid, slippage_paid)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
        ON CONFLICT (id) DO UPDATE SET
            exit_price = EXCLUDED.exit_price,
            closed_at = EXCLUDED.closed_at,
            status = EXCLUDED.status,
            realized_pnl_gross = EXCLUDED.realized_pnl_gross,
            realized_pnl_net = EXCLUDED.realized_pnl_net,
            fees_paid = EXCLUDED.fees_paid,
            spread_paid = EXCLUDED.spread_paid,
            slippage_paid = EXCLUDED.slippage_paid
        "#,
    )
    .bind(position.id)
    .bind(position.instrument_id.0)
    .bind(position.strategy_id.as_str())
    .bind(side_to_str(position.side))
    .bind(position.quantity)
    .bind(position.entry_price)
    .bind(position.exit_price)
    .bind(position.opened_at)
    .bind(position.closed_at)
    .bind(position_status_to_str(position.status))
    .bind(position.realized_pnl_gross)
    .bind(position.realized_pnl_net)
    .bind(position.fees_paid)
    .bind(position.spread_paid)
    .bind(position.slippage_paid)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_open(pool: &PgPool) -> Result<Vec<Position>, PersistenceError> {
    let rows: Vec<PositionRow> = sqlx::query_as(
        r#"SELECT id, instrument_id, strategy_id, side, quantity, entry_price, exit_price,
                  opened_at, closed_at, status, realized_pnl_gross, realized_pnl_net, fees_paid,
                  spread_paid, slippage_paid
           FROM positions WHERE status = 'open'"#,
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter().map(row_to_position).collect()
}

/// Posições fechadas, de todas as estratégias — a UI de observabilidade
/// usa isso (via `analytics::compute_performance`) para calcular max
/// drawdown sobre a carteira inteira e, agrupando por `strategy_id`, o
/// desempenho por estratégia.
pub async fn list_closed(pool: &PgPool) -> Result<Vec<Position>, PersistenceError> {
    let rows: Vec<PositionRow> = sqlx::query_as(
        r#"SELECT id, instrument_id, strategy_id, side, quantity, entry_price, exit_price,
                  opened_at, closed_at, status, realized_pnl_gross, realized_pnl_net, fees_paid,
                  spread_paid, slippage_paid
           FROM positions WHERE status = 'closed'"#,
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter().map(row_to_position).collect()
}
