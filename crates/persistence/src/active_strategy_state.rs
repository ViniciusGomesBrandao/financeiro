use chrono::{DateTime, Utc};
use domain::InstrumentId;
use sqlx::PgPool;

use crate::error::PersistenceError;

#[derive(Debug, Clone)]
pub struct ActiveStrategyState {
    pub robot_id: String,
    pub instrument_id: InstrumentId,
    pub selected_strategy_id: Option<String>,
    pub evaluated_at: DateTime<Utc>,
}

pub async fn upsert_active_state(
    pool: &PgPool,
    robot_id: &str,
    instrument_id: InstrumentId,
    selected_strategy_id: Option<&str>,
    evaluated_at: DateTime<Utc>,
) -> Result<(), PersistenceError> {
    sqlx::query(
        r#"
        INSERT INTO active_strategy_state (robot_id, instrument_id, selected_strategy_id, evaluated_at)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (robot_id) DO UPDATE SET
            instrument_id = EXCLUDED.instrument_id,
            selected_strategy_id = EXCLUDED.selected_strategy_id,
            evaluated_at = EXCLUDED.evaluated_at
        "#,
    )
    .bind(robot_id)
    .bind(instrument_id.0)
    .bind(selected_strategy_id)
    .bind(evaluated_at)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct ActiveStrategyStateRow {
    robot_id: String,
    instrument_id: uuid::Uuid,
    selected_strategy_id: Option<String>,
    evaluated_at: DateTime<Utc>,
}

pub async fn list_active_states(
    pool: &PgPool,
) -> Result<Vec<ActiveStrategyState>, PersistenceError> {
    let rows: Vec<ActiveStrategyStateRow> = sqlx::query_as(
        r#"
        SELECT robot_id, instrument_id, selected_strategy_id, evaluated_at
        FROM active_strategy_state
        ORDER BY evaluated_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| ActiveStrategyState {
            robot_id: row.robot_id,
            instrument_id: InstrumentId(row.instrument_id),
            selected_strategy_id: row.selected_strategy_id,
            evaluated_at: row.evaluated_at,
        })
        .collect())
}

pub async fn get_for_robot(
    pool: &PgPool,
    robot_id: &str,
) -> Result<Option<ActiveStrategyState>, PersistenceError> {
    let row: Option<ActiveStrategyStateRow> = sqlx::query_as(
        r#"
        SELECT robot_id, instrument_id, selected_strategy_id, evaluated_at
        FROM active_strategy_state
        WHERE robot_id = $1
        "#,
    )
    .bind(robot_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| ActiveStrategyState {
        robot_id: row.robot_id,
        instrument_id: InstrumentId(row.instrument_id),
        selected_strategy_id: row.selected_strategy_id,
        evaluated_at: row.evaluated_at,
    }))
}
