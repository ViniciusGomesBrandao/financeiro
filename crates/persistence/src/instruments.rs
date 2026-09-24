use domain::{Asset, Exchange, Instrument, InstrumentId, Symbol};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::codec::{
    asset_class_from_str, asset_class_to_str, market_type_from_str, market_type_to_str,
};
use crate::error::PersistenceError;

#[derive(sqlx::FromRow)]
struct InstrumentRow {
    id: Uuid,
    base_asset: String,
    quote_asset: String,
    asset_class: String,
    exchange: String,
    market_type: String,
    tick_size: Decimal,
    lot_size: Decimal,
    min_quantity: Decimal,
    min_notional: Decimal,
}

fn exchange_to_str(v: &Exchange) -> String {
    match v {
        Exchange::Binance => "binance".to_string(),
        Exchange::Other(name) => name.clone(),
    }
}

fn exchange_from_str(v: &str) -> Exchange {
    match v {
        "binance" => Exchange::Binance,
        other => Exchange::Other(other.to_string()),
    }
}

fn row_to_instrument(row: InstrumentRow) -> Result<Instrument, PersistenceError> {
    Ok(Instrument {
        id: InstrumentId(row.id),
        symbol: Symbol::from_pair(
            &Asset::new(&row.base_asset).map_err(|e| {
                PersistenceError::Database(sqlx::Error::Decode(e.to_string().into()))
            })?,
            &Asset::new(&row.quote_asset).map_err(|e| {
                PersistenceError::Database(sqlx::Error::Decode(e.to_string().into()))
            })?,
        ),
        base_asset: Asset::new(&row.base_asset)
            .map_err(|e| PersistenceError::Database(sqlx::Error::Decode(e.to_string().into())))?,
        quote_asset: Asset::new(&row.quote_asset)
            .map_err(|e| PersistenceError::Database(sqlx::Error::Decode(e.to_string().into())))?,
        asset_class: asset_class_from_str(&row.asset_class)?,
        exchange: exchange_from_str(&row.exchange),
        market_type: market_type_from_str(&row.market_type)?,
        tick_size: row.tick_size,
        lot_size: row.lot_size,
        min_quantity: row.min_quantity,
        min_notional: row.min_notional,
    })
}

/// Insere `instrument`, ou atualiza as colunas de metadados derivados da
/// exchange (tick size, lot size, mínimos) caso já exista uma linha com a
/// mesma chave natural `(symbol, exchange, market_type)`.
///
/// O Postgres — não o chamador — é a autoridade sobre o `InstrumentId`: a
/// coluna `id` está deliberadamente *fora* da cláusula `DO UPDATE SET`,
/// então em caso de conflito o id da linha existente é preservado, e o
/// `RETURNING id` devolve o id que passa a ser autoritativo (o recém
/// inserido, ou o preexistente). `Instrument::new` atribui um id aleatório
/// novo a cada chamada — inclusive a cada reinício do processo que busca o
/// mesmo instrumento na exchange —, então os chamadores devem sobrescrever
/// seu `Instrument::id` em memória com este valor de retorno, em vez de
/// confiar no que passaram. Ver `docs/architecture.md` ADR-9.
pub async fn upsert(
    pool: &PgPool,
    instrument: &Instrument,
) -> Result<InstrumentId, PersistenceError> {
    let id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO instruments
            (id, symbol, base_asset, quote_asset, asset_class, exchange, market_type,
             tick_size, lot_size, min_quantity, min_notional)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        ON CONFLICT (symbol, exchange, market_type) DO UPDATE SET
            tick_size = EXCLUDED.tick_size,
            lot_size = EXCLUDED.lot_size,
            min_quantity = EXCLUDED.min_quantity,
            min_notional = EXCLUDED.min_notional
        RETURNING id
        "#,
    )
    .bind(instrument.id.0)
    .bind(instrument.symbol.as_str())
    .bind(instrument.base_asset.as_str())
    .bind(instrument.quote_asset.as_str())
    .bind(asset_class_to_str(instrument.asset_class))
    .bind(exchange_to_str(&instrument.exchange))
    .bind(market_type_to_str(instrument.market_type))
    .bind(instrument.tick_size)
    .bind(instrument.lot_size)
    .bind(instrument.min_quantity)
    .bind(instrument.min_notional)
    .fetch_one(pool)
    .await?;
    Ok(InstrumentId(id))
}

pub async fn find_by_id(
    pool: &PgPool,
    id: InstrumentId,
) -> Result<Option<Instrument>, PersistenceError> {
    let row: Option<InstrumentRow> = sqlx::query_as(
        r#"SELECT id, base_asset, quote_asset, asset_class, exchange, market_type,
                  tick_size, lot_size, min_quantity, min_notional
           FROM instruments WHERE id = $1"#,
    )
    .bind(id.0)
    .fetch_optional(pool)
    .await?;

    row.map(row_to_instrument).transpose()
}

pub async fn list_all(pool: &PgPool) -> Result<Vec<Instrument>, PersistenceError> {
    let rows: Vec<InstrumentRow> = sqlx::query_as(
        r#"SELECT id, base_asset, quote_asset, asset_class, exchange, market_type,
                  tick_size, lot_size, min_quantity, min_notional
           FROM instruments ORDER BY symbol"#,
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter().map(row_to_instrument).collect()
}
