use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;

use crate::error::PersistenceError;

/// Robô operacional configurado pelo usuário — agrupa candidatas de
/// estratégia para um símbolo e capital fictício de referência.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationalRobot {
    pub id: String,
    pub name: String,
    pub symbol: String,
    pub timeframe: String,
    pub candidate_kinds: Vec<String>,
    pub paper_capital: Decimal,
    pub status: RobotStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RobotStatus {
    Running,
    Stopped,
}

impl RobotStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, PersistenceError> {
        match raw {
            "running" => Ok(Self::Running),
            "stopped" => Ok(Self::Stopped),
            other => Err(PersistenceError::Database(sqlx::Error::Decode(
                format!("unknown robot status: {other}").into(),
            ))),
        }
    }
}

#[derive(sqlx::FromRow)]
struct OperationalRobotRow {
    id: String,
    name: String,
    symbol: String,
    timeframe: String,
    candidate_kinds: Vec<String>,
    paper_capital: Decimal,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn row_to_robot(row: OperationalRobotRow) -> Result<OperationalRobot, PersistenceError> {
    Ok(OperationalRobot {
        id: row.id,
        name: row.name,
        symbol: row.symbol,
        timeframe: row.timeframe,
        candidate_kinds: row.candidate_kinds,
        paper_capital: row.paper_capital,
        status: RobotStatus::parse(&row.status)?,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// Identificador estável de instância de estratégia pertencente a um robô.
pub fn strategy_instance_id(robot_id: &str, kind: &str) -> String {
    format!("{robot_id}::{kind}")
}

pub async fn insert(pool: &PgPool, robot: &OperationalRobot) -> Result<(), PersistenceError> {
    sqlx::query(
        r#"
        INSERT INTO operational_robots
            (id, name, symbol, timeframe, candidate_kinds, paper_capital, status, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(&robot.id)
    .bind(&robot.name)
    .bind(&robot.symbol)
    .bind(&robot.timeframe)
    .bind(&robot.candidate_kinds)
    .bind(robot.paper_capital)
    .bind(robot.status.as_str())
    .bind(robot.created_at)
    .bind(robot.updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_all(pool: &PgPool) -> Result<Vec<OperationalRobot>, PersistenceError> {
    let rows: Vec<OperationalRobotRow> = sqlx::query_as(
        r#"
        SELECT id, name, symbol, timeframe, candidate_kinds, paper_capital, status, created_at, updated_at
        FROM operational_robots
        ORDER BY created_at ASC
        "#,
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(row_to_robot).collect()
}

pub async fn list_running(pool: &PgPool) -> Result<Vec<OperationalRobot>, PersistenceError> {
    let rows: Vec<OperationalRobotRow> = sqlx::query_as(
        r#"
        SELECT id, name, symbol, timeframe, candidate_kinds, paper_capital, status, created_at, updated_at
        FROM operational_robots
        WHERE status = 'running'
        ORDER BY created_at ASC
        "#,
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(row_to_robot).collect()
}

pub async fn get(pool: &PgPool, id: &str) -> Result<Option<OperationalRobot>, PersistenceError> {
    let row: Option<OperationalRobotRow> = sqlx::query_as(
        r#"
        SELECT id, name, symbol, timeframe, candidate_kinds, paper_capital, status, created_at, updated_at
        FROM operational_robots
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.map(row_to_robot).transpose()
}

pub async fn find_running_by_symbol(
    pool: &PgPool,
    symbol: &str,
) -> Result<Option<OperationalRobot>, PersistenceError> {
    let row: Option<OperationalRobotRow> = sqlx::query_as(
        r#"
        SELECT id, name, symbol, timeframe, candidate_kinds, paper_capital, status, created_at, updated_at
        FROM operational_robots
        WHERE symbol = $1 AND status = 'running'
        ORDER BY created_at ASC
        LIMIT 1
        "#,
    )
    .bind(symbol)
    .fetch_optional(pool)
    .await?;
    row.map(row_to_robot).transpose()
}

pub async fn set_status(
    pool: &PgPool,
    id: &str,
    status: RobotStatus,
) -> Result<bool, PersistenceError> {
    let result = sqlx::query(
        r#"
        UPDATE operational_robots
        SET status = $2, updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(status.as_str())
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
