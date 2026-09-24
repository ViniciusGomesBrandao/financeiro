use chrono::{DateTime, Utc};
use domain::InstrumentId;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::PersistenceError;

#[derive(Debug, Clone)]
pub struct JudgeEvaluationRecord {
    pub id: Uuid,
    pub instrument_id: InstrumentId,
    pub robot_id: Option<String>,
    pub evaluated_at: DateTime<Utc>,
    pub selected_strategy_id: Option<String>,
    pub decisions_json: Value,
}

#[derive(Debug, Clone)]
pub struct StrategySwitchRecord {
    pub id: Uuid,
    pub instrument_id: InstrumentId,
    pub robot_id: Option<String>,
    pub previous_strategy_id: Option<String>,
    pub new_strategy_id: Option<String>,
    pub reason_json: Value,
    pub switched_at: DateTime<Utc>,
}

pub async fn insert_evaluation(
    pool: &PgPool,
    instrument_id: InstrumentId,
    robot_id: Option<&str>,
    evaluated_at: DateTime<Utc>,
    selected_strategy_id: Option<&str>,
    decisions_json: &Value,
) -> Result<Uuid, PersistenceError> {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO judge_evaluations
            (id, instrument_id, robot_id, evaluated_at, selected_strategy_id, decisions_json)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(id)
    .bind(instrument_id.0)
    .bind(robot_id)
    .bind(evaluated_at)
    .bind(selected_strategy_id)
    .bind(decisions_json)
    .execute(pool)
    .await?;
    Ok(id)
}

pub async fn insert_switch(
    pool: &PgPool,
    instrument_id: InstrumentId,
    robot_id: Option<&str>,
    previous_strategy_id: Option<&str>,
    new_strategy_id: Option<&str>,
    reason_json: &Value,
    switched_at: DateTime<Utc>,
) -> Result<Uuid, PersistenceError> {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO strategy_switches
            (id, instrument_id, robot_id, previous_strategy_id, new_strategy_id, reason_json, switched_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(id)
    .bind(instrument_id.0)
    .bind(robot_id)
    .bind(previous_strategy_id)
    .bind(new_strategy_id)
    .bind(reason_json)
    .bind(switched_at)
    .execute(pool)
    .await?;
    Ok(id)
}

#[derive(sqlx::FromRow)]
struct JudgeEvaluationRow {
    id: Uuid,
    instrument_id: Uuid,
    robot_id: Option<String>,
    evaluated_at: DateTime<Utc>,
    selected_strategy_id: Option<String>,
    decisions_json: Value,
}

pub async fn latest_evaluations(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<JudgeEvaluationRecord>, PersistenceError> {
    let rows: Vec<JudgeEvaluationRow> = sqlx::query_as(
        r#"
        SELECT id, instrument_id, robot_id, evaluated_at, selected_strategy_id, decisions_json
        FROM judge_evaluations
        ORDER BY evaluated_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(JudgeEvaluationRecord {
                id: row.id,
                instrument_id: InstrumentId(row.instrument_id),
                robot_id: row.robot_id,
                evaluated_at: row.evaluated_at,
                selected_strategy_id: row.selected_strategy_id,
                decisions_json: row.decisions_json,
            })
        })
        .collect()
}

pub async fn latest_evaluation_for_instrument(
    pool: &PgPool,
    instrument_id: InstrumentId,
) -> Result<Option<JudgeEvaluationRecord>, PersistenceError> {
    let row: Option<JudgeEvaluationRow> = sqlx::query_as(
        r#"
        SELECT id, instrument_id, robot_id, evaluated_at, selected_strategy_id, decisions_json
        FROM judge_evaluations
        WHERE instrument_id = $1
        ORDER BY evaluated_at DESC
        LIMIT 1
        "#,
    )
    .bind(instrument_id.0)
    .fetch_optional(pool)
    .await?;

    row.map(|row| {
        Ok(JudgeEvaluationRecord {
            id: row.id,
            instrument_id: InstrumentId(row.instrument_id),
            robot_id: row.robot_id,
            evaluated_at: row.evaluated_at,
            selected_strategy_id: row.selected_strategy_id,
            decisions_json: row.decisions_json,
        })
    })
    .transpose()
}

#[derive(sqlx::FromRow)]
struct StrategySwitchRow {
    id: Uuid,
    instrument_id: Uuid,
    robot_id: Option<String>,
    previous_strategy_id: Option<String>,
    new_strategy_id: Option<String>,
    reason_json: Value,
    switched_at: DateTime<Utc>,
}

pub async fn list_switches(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<StrategySwitchRecord>, PersistenceError> {
    let rows: Vec<StrategySwitchRow> = sqlx::query_as(
        r#"
        SELECT id, instrument_id, robot_id, previous_strategy_id, new_strategy_id, reason_json, switched_at
        FROM strategy_switches
        ORDER BY switched_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    rows.into_iter().map(row_to_switch).collect()
}

pub async fn list_evaluations_for_robot(
    pool: &PgPool,
    robot_id: &str,
    limit: i64,
) -> Result<Vec<JudgeEvaluationRecord>, PersistenceError> {
    let rows: Vec<JudgeEvaluationRow> = sqlx::query_as(
        r#"
        SELECT id, instrument_id, robot_id, evaluated_at, selected_strategy_id, decisions_json
        FROM judge_evaluations
        WHERE robot_id = $1
        ORDER BY evaluated_at DESC
        LIMIT $2
        "#,
    )
    .bind(robot_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(JudgeEvaluationRecord {
                id: row.id,
                instrument_id: InstrumentId(row.instrument_id),
                robot_id: row.robot_id,
                evaluated_at: row.evaluated_at,
                selected_strategy_id: row.selected_strategy_id,
                decisions_json: row.decisions_json,
            })
        })
        .collect()
}

pub async fn list_switches_for_robot(
    pool: &PgPool,
    robot_id: &str,
    limit: i64,
) -> Result<Vec<StrategySwitchRecord>, PersistenceError> {
    let rows: Vec<StrategySwitchRow> = sqlx::query_as(
        r#"
        SELECT id, instrument_id, robot_id, previous_strategy_id, new_strategy_id, reason_json, switched_at
        FROM strategy_switches
        WHERE robot_id = $1
        ORDER BY switched_at DESC
        LIMIT $2
        "#,
    )
    .bind(robot_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    rows.into_iter().map(row_to_switch).collect()
}

fn row_to_switch(row: StrategySwitchRow) -> Result<StrategySwitchRecord, PersistenceError> {
    Ok(StrategySwitchRecord {
        id: row.id,
        instrument_id: InstrumentId(row.instrument_id),
        robot_id: row.robot_id,
        previous_strategy_id: row.previous_strategy_id,
        new_strategy_id: row.new_strategy_id,
        reason_json: row.reason_json,
        switched_at: row.switched_at,
    })
}
