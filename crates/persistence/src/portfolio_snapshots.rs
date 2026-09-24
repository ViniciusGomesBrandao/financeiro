use chrono::{DateTime, Utc};
use domain::PortfolioSnapshot;
use rust_decimal::Decimal;
use sqlx::PgPool;

use crate::error::PersistenceError;

#[derive(sqlx::FromRow)]
struct SnapshotRow {
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

fn row_to_snapshot(row: SnapshotRow) -> PortfolioSnapshot {
    PortfolioSnapshot {
        timestamp: row.timestamp,
        cash: row.cash,
        equity: row.equity,
        realized_pnl: row.realized_pnl,
        unrealized_pnl: row.unrealized_pnl,
        open_positions_count: row.open_positions_count.max(0) as u32,
        exposure_ratio: row.exposure_ratio,
        return_pct: row.return_pct,
        realized_pnl_today: row.realized_pnl_today,
    }
}

pub async fn insert(pool: &PgPool, snapshot: &PortfolioSnapshot) -> Result<(), PersistenceError> {
    sqlx::query(
        r#"
        INSERT INTO portfolio_snapshots
            ("timestamp", cash, equity, realized_pnl, unrealized_pnl, open_positions_count,
             exposure_ratio, return_pct, realized_pnl_today)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
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

pub async fn list_recent(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<PortfolioSnapshot>, PersistenceError> {
    let rows: Vec<SnapshotRow> = sqlx::query_as(
        r#"SELECT "timestamp", cash, equity, realized_pnl, unrealized_pnl, open_positions_count,
                  exposure_ratio, return_pct, realized_pnl_today
           FROM portfolio_snapshots ORDER BY "timestamp" DESC LIMIT $1"#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(row_to_snapshot).collect())
}

/// Snapshot mais recente, se existir — o que a UI de observabilidade usa
/// como "estado atual" do portfólio.
pub async fn latest(pool: &PgPool) -> Result<Option<PortfolioSnapshot>, PersistenceError> {
    Ok(list_recent(pool, 1).await?.into_iter().next())
}
