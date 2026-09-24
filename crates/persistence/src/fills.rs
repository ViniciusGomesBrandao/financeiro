use chrono::{DateTime, Utc};
use domain::Fill;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::codec::{liquidity_from_str, liquidity_to_str};
use crate::error::PersistenceError;

#[derive(sqlx::FromRow)]
struct FillRow {
    id: Uuid,
    order_id: Uuid,
    price: Decimal,
    quantity: Decimal,
    fee: Decimal,
    spread_cost: Decimal,
    slippage_cost: Decimal,
    liquidity: String,
    executed_at: DateTime<Utc>,
}

fn row_to_fill(row: FillRow) -> Result<Fill, PersistenceError> {
    Ok(Fill {
        id: row.id,
        order_id: row.order_id,
        price: row.price,
        quantity: row.quantity,
        fee: row.fee,
        spread_cost: row.spread_cost,
        slippage_cost: row.slippage_cost,
        liquidity: liquidity_from_str(&row.liquidity)?,
        executed_at: row.executed_at,
    })
}

pub async fn insert(pool: &PgPool, fill: &Fill) -> Result<(), PersistenceError> {
    sqlx::query(
        r#"
        INSERT INTO fills
            (id, order_id, price, quantity, fee, spread_cost, slippage_cost, liquidity, executed_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(fill.id)
    .bind(fill.order_id)
    .bind(fill.price)
    .bind(fill.quantity)
    .bind(fill.fee)
    .bind(fill.spread_cost)
    .bind(fill.slippage_cost)
    .bind(liquidity_to_str(fill.liquidity))
    .bind(fill.executed_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_for_order(pool: &PgPool, order_id: Uuid) -> Result<Vec<Fill>, PersistenceError> {
    let rows: Vec<FillRow> = sqlx::query_as(
        r#"SELECT id, order_id, price, quantity, fee, spread_cost, slippage_cost, liquidity, executed_at
           FROM fills WHERE order_id = $1 ORDER BY executed_at"#,
    )
    .bind(order_id)
    .fetch_all(pool)
    .await?;

    rows.into_iter().map(row_to_fill).collect()
}
