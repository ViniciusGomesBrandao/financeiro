use chrono::{DateTime, Utc};
use domain::InstrumentId;
use rust_decimal::Decimal;
use sqlx::PgPool;

use crate::error::PersistenceError;

/// Último preço conhecido de um instrumento, atualizado pelo pipeline live
/// a cada candle fechada. Existe apenas para a UI de observabilidade
/// (processo separado do pipeline) conseguir mostrar "preço atual" lendo
/// do Postgres — não é um histórico de ticks.
#[derive(Debug, Clone)]
pub struct LatestPrice {
    pub instrument_id: InstrumentId,
    pub price: Decimal,
    pub updated_at: DateTime<Utc>,
}

pub async fn upsert(
    pool: &PgPool,
    instrument_id: InstrumentId,
    price: Decimal,
    updated_at: DateTime<Utc>,
) -> Result<(), PersistenceError> {
    sqlx::query(
        r#"
        INSERT INTO latest_prices (instrument_id, price, updated_at)
        VALUES ($1, $2, $3)
        ON CONFLICT (instrument_id) DO UPDATE SET
            price = EXCLUDED.price,
            updated_at = EXCLUDED.updated_at
        "#,
    )
    .bind(instrument_id.0)
    .bind(price)
    .bind(updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_all(pool: &PgPool) -> Result<Vec<LatestPrice>, PersistenceError> {
    let rows = sqlx::query_as::<_, LatestPriceRow>(
        "SELECT instrument_id, price, updated_at FROM latest_prices",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(LatestPriceRow::into_domain).collect())
}

#[derive(sqlx::FromRow)]
struct LatestPriceRow {
    instrument_id: uuid::Uuid,
    price: Decimal,
    updated_at: DateTime<Utc>,
}

impl LatestPriceRow {
    fn into_domain(self) -> LatestPrice {
        LatestPrice {
            instrument_id: InstrumentId(self.instrument_id),
            price: self.price,
            updated_at: self.updated_at,
        }
    }
}
