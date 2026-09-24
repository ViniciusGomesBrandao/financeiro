use chrono::{DateTime, Utc};
use domain::{InstrumentId, Order, SignalId, StrategyId};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::codec::{
    order_status_from_str, order_status_to_str, order_type_from_str, order_type_to_str,
    side_from_str, side_to_str,
};
use crate::error::PersistenceError;

#[derive(sqlx::FromRow)]
struct OrderRow {
    id: Uuid,
    instrument_id: Uuid,
    strategy_id: String,
    signal_id: Option<Uuid>,
    side: String,
    order_type: String,
    quantity: Decimal,
    limit_price: Option<Decimal>,
    status: String,
    filled_quantity: Decimal,
    average_fill_price: Option<Decimal>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn row_to_order(row: OrderRow) -> Result<Order, PersistenceError> {
    Ok(Order {
        id: row.id,
        instrument_id: InstrumentId(row.instrument_id),
        strategy_id: StrategyId::new(row.strategy_id)
            .map_err(|e| PersistenceError::Database(sqlx::Error::Decode(e.to_string().into())))?,
        signal_id: row.signal_id.map(SignalId),
        side: side_from_str(&row.side)?,
        order_type: order_type_from_str(&row.order_type)?,
        quantity: row.quantity,
        limit_price: row.limit_price,
        status: order_status_from_str(&row.status)?,
        filled_quantity: row.filled_quantity,
        average_fill_price: row.average_fill_price,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

pub async fn insert(pool: &PgPool, order: &Order) -> Result<(), PersistenceError> {
    sqlx::query(
        r#"
        INSERT INTO orders
            (id, instrument_id, strategy_id, signal_id, side, order_type, quantity,
             limit_price, status, filled_quantity, average_fill_price, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
        "#,
    )
    .bind(order.id)
    .bind(order.instrument_id.0)
    .bind(order.strategy_id.as_str())
    .bind(order.signal_id.map(|s| s.0))
    .bind(side_to_str(order.side))
    .bind(order_type_to_str(order.order_type))
    .bind(order.quantity)
    .bind(order.limit_price)
    .bind(order_status_to_str(order.status))
    .bind(order.filled_quantity)
    .bind(order.average_fill_price)
    .bind(order.created_at)
    .bind(order.updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_for_instrument(
    pool: &PgPool,
    instrument_id: InstrumentId,
    limit: i64,
) -> Result<Vec<Order>, PersistenceError> {
    let rows: Vec<OrderRow> = sqlx::query_as(
        r#"SELECT id, instrument_id, strategy_id, signal_id, side, order_type, quantity,
                  limit_price, status, filled_quantity, average_fill_price, created_at, updated_at
           FROM orders WHERE instrument_id = $1 ORDER BY created_at DESC LIMIT $2"#,
    )
    .bind(instrument_id.0)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    rows.into_iter().map(row_to_order).collect()
}
