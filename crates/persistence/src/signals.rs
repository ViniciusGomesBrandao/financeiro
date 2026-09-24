use chrono::{DateTime, Utc};
use domain::{InstrumentId, Signal, SignalId, StrategyId, Timeframe};
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::codec::{signal_direction_from_str, signal_direction_to_str};
use crate::error::PersistenceError;

#[derive(sqlx::FromRow)]
struct SignalRow {
    id: Uuid,
    strategy_id: String,
    instrument_id: Uuid,
    direction: String,
    confidence: f64,
    expected_return: Option<Decimal>,
    time_horizon: Option<String>,
    metadata: Value,
    created_at: DateTime<Utc>,
}

fn timeframe_from_str(v: &str) -> Option<Timeframe> {
    match v {
        "1m" => Some(Timeframe::M1),
        "5m" => Some(Timeframe::M5),
        "15m" => Some(Timeframe::M15),
        "30m" => Some(Timeframe::M30),
        "1h" => Some(Timeframe::H1),
        "4h" => Some(Timeframe::H4),
        "1d" => Some(Timeframe::D1),
        "1w" => Some(Timeframe::W1),
        _ => None,
    }
}

fn row_to_signal(row: SignalRow) -> Result<Signal, PersistenceError> {
    Ok(Signal {
        id: SignalId(row.id),
        strategy_id: StrategyId::new(row.strategy_id)
            .map_err(|e| PersistenceError::Database(sqlx::Error::Decode(e.to_string().into())))?,
        instrument_id: InstrumentId(row.instrument_id),
        direction: signal_direction_from_str(&row.direction)?,
        confidence: row.confidence,
        timestamp: row.created_at,
        expected_return: row.expected_return,
        time_horizon: row.time_horizon.as_deref().and_then(timeframe_from_str),
        metadata: row.metadata,
    })
}

pub async fn insert(pool: &PgPool, signal: &Signal) -> Result<(), PersistenceError> {
    sqlx::query(
        r#"
        INSERT INTO signals
            (id, strategy_id, instrument_id, direction, confidence, expected_return, time_horizon, metadata, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(signal.id.0)
    .bind(signal.strategy_id.as_str())
    .bind(signal.instrument_id.0)
    .bind(signal_direction_to_str(signal.direction))
    .bind(signal.confidence)
    .bind(signal.expected_return)
    .bind(signal.time_horizon.map(|t| t.as_str()))
    .bind(&signal.metadata)
    .bind(signal.timestamp)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_recent_for_strategy(
    pool: &PgPool,
    strategy_id: &StrategyId,
    limit: i64,
) -> Result<Vec<Signal>, PersistenceError> {
    let rows: Vec<SignalRow> = sqlx::query_as(
        r#"SELECT id, strategy_id, instrument_id, direction, confidence, expected_return,
                  time_horizon, metadata, created_at
           FROM signals WHERE strategy_id = $1 ORDER BY created_at DESC LIMIT $2"#,
    )
    .bind(strategy_id.as_str())
    .bind(limit)
    .fetch_all(pool)
    .await?;

    rows.into_iter().map(row_to_signal).collect()
}
