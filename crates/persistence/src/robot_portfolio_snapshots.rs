use chrono::{DateTime, Utc};
use domain::PortfolioSnapshot;
use rust_decimal::Decimal;
use sqlx::PgPool;

use crate::error::PersistenceError;

#[derive(Debug, Clone)]
pub struct RobotPortfolioSnapshot {
    pub robot_id: String,
    pub snapshot: PortfolioSnapshot,
}

#[derive(sqlx::FromRow)]
struct Row {
    robot_id: String,
    timestamp: DateTime<Utc>,
    cash: Decimal,
    equity: Decimal,
    realized_pnl: Decimal,
    unrealized_pnl: Decimal,
    open_positions_count: i32,
    exposure_ratio: Decimal,
    return_pct: Decimal,
    realized_pnl_today: Decimal,
}

fn row_to_record(row: Row) -> RobotPortfolioSnapshot {
    RobotPortfolioSnapshot {
        robot_id: row.robot_id,
        snapshot: PortfolioSnapshot {
            timestamp: row.timestamp,
            cash: row.cash,
            equity: row.equity,
            realized_pnl: row.realized_pnl,
            unrealized_pnl: row.unrealized_pnl,
            open_positions_count: row.open_positions_count.max(0) as u32,
            exposure_ratio: row.exposure_ratio,
            return_pct: row.return_pct,
            realized_pnl_today: row.realized_pnl_today,
        },
    }
}

pub async fn insert(
    pool: &PgPool,
    robot_id: &str,
    snapshot: &PortfolioSnapshot,
) -> Result<(), PersistenceError> {
    sqlx::query(
        r#"
        INSERT INTO robot_portfolio_snapshots
            (robot_id, "timestamp", cash, equity, realized_pnl, unrealized_pnl,
             open_positions_count, exposure_ratio, return_pct, realized_pnl_today)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
    )
    .bind(robot_id)
    .bind(snapshot.timestamp)
    .bind(snapshot.cash)
    .bind(snapshot.equity)
    .bind(snapshot.realized_pnl)
    .bind(snapshot.unrealized_pnl)
    .bind(snapshot.open_positions_count as i32)
    .bind(snapshot.exposure_ratio)
    .bind(snapshot.return_pct)
    .bind(snapshot.realized_pnl_today)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn latest_for_robot(
    pool: &PgPool,
    robot_id: &str,
) -> Result<Option<PortfolioSnapshot>, PersistenceError> {
    let row: Option<Row> = sqlx::query_as(
        r#"
        SELECT robot_id, "timestamp", cash, equity, realized_pnl, unrealized_pnl,
               open_positions_count, exposure_ratio, return_pct, realized_pnl_today
        FROM robot_portfolio_snapshots
        WHERE robot_id = $1
        ORDER BY "timestamp" DESC
        LIMIT 1
        "#,
    )
    .bind(robot_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(row_to_record).map(|r| r.snapshot))
}

pub async fn latest_all(pool: &PgPool) -> Result<Vec<RobotPortfolioSnapshot>, PersistenceError> {
    // Um snapshot por robô: o mais recente de cada um (DISTINCT ON).
    let rows: Vec<Row> = sqlx::query_as(
        r#"
        SELECT DISTINCT ON (robot_id)
            robot_id, "timestamp", cash, equity, realized_pnl, unrealized_pnl,
            open_positions_count, exposure_ratio, return_pct, realized_pnl_today
        FROM robot_portfolio_snapshots
        ORDER BY robot_id, "timestamp" DESC
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_to_record).collect())
}

/// Histórico recente de snapshots de um robô (ASC por tempo) — para curva
/// de equity no dashboard. Não cria dado novo; só lê o que o pipeline
/// já grava a cada candle.
pub async fn list_recent_for_robot(
    pool: &PgPool,
    robot_id: &str,
    limit: i64,
) -> Result<Vec<RobotPortfolioSnapshot>, PersistenceError> {
    let rows: Vec<Row> = sqlx::query_as(
        r#"
        SELECT robot_id, "timestamp", cash, equity, realized_pnl, unrealized_pnl,
               open_positions_count, exposure_ratio, return_pct, realized_pnl_today
        FROM robot_portfolio_snapshots
        WHERE robot_id = $1
        ORDER BY "timestamp" DESC
        LIMIT $2
        "#,
    )
    .bind(robot_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    let mut out: Vec<_> = rows.into_iter().map(row_to_record).collect();
    out.reverse();
    Ok(out)
}
