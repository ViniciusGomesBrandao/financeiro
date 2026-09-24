//! Persistência local em Parquet para dado de microestrutura — mesmo
//! princípio de `historical_data::store` (preço/quantidade como
//! `Decimal128` de escala fixa, nunca `f64`, pela mesma razão: nenhum
//! ponto flutuante binário representa exatamente todo `Decimal` base-10 —
//! ver o doc daquele módulo), mas particionado por **símbolo + dia UTC**,
//! não por símbolo+timeframe: volume de tick data não cabe em "um arquivo
//! por símbolo".
//!
//! Dentro de um dia, cada chamada de `write_*` grava um **novo arquivo
//! imutável** (`{data_dir}/{symbol}/{kind}/{yyyy-mm-dd}/{HHMMSS_nanos}.parquet`)
//! em vez de manter um `ArrowWriter` aberto o dia inteiro — um `ArrowWriter`
//! nunca fechado (processo derrubado no meio do dia) produz um arquivo sem
//! footer, ilegível; com um arquivo novo por flush, só o buffer do
//! intervalo desde o último flush bem-sucedido fica em risco, nunca o dia
//! inteiro. `read_day` faz o inverso: glob de todos os arquivos do dia +
//! concatenação, ordenado pela chave de sequência de cada schema.
//!
//! Quatro schemas, todos "long format" (uma linha por unidade atômica) —
//! inclusive os deltas de book, que têm um número variável de níveis por
//! evento: uma linha por nível atualizado, agrupável de volta por
//! `(first_update_id, final_update_id)`.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::array::{Array, BooleanArray, Decimal128Array, Float64Array, Int64Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use domain::Side;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use rust_decimal::Decimal;

use crate::error::MicrostructureError;

const DECIMAL_SCALE: u32 = 8;
const DECIMAL_PRECISION: u8 = 38;

fn decimal_field(name: &str) -> Field {
    Field::new(
        name,
        DataType::Decimal128(DECIMAL_PRECISION, DECIMAL_SCALE as i8),
        false,
    )
}

fn decimal_to_scaled_mantissa(
    value: Decimal,
    column: &'static str,
    row: usize,
) -> Result<i128, MicrostructureError> {
    if value.scale() > DECIMAL_SCALE {
        return Err(MicrostructureError::ScaleTooLarge {
            column,
            row,
            value: value.to_string(),
            max_scale: DECIMAL_SCALE,
        });
    }
    let mut rescaled = value;
    rescaled.rescale(DECIMAL_SCALE);
    Ok(rescaled.mantissa())
}

fn scaled_mantissa_to_decimal(mantissa: i128) -> Decimal {
    Decimal::from_i128_with_scale(mantissa, DECIMAL_SCALE)
}

fn decimal_array(values: Vec<i128>) -> Result<Decimal128Array, MicrostructureError> {
    Decimal128Array::from(values)
        .with_precision_and_scale(DECIMAL_PRECISION, DECIMAL_SCALE as i8)
        .map_err(MicrostructureError::from)
}

fn millis_to_utc(ms: i64) -> Result<DateTime<Utc>, MicrostructureError> {
    Utc.timestamp_millis_opt(ms)
        .single()
        .ok_or(MicrostructureError::MalformedColumn {
            column: "*_ms",
            row: 0,
        })
}

fn column_i64<'a>(
    batch: &'a RecordBatch,
    index: usize,
    name: &'static str,
) -> Result<&'a Int64Array, MicrostructureError> {
    batch
        .column(index)
        .as_any()
        .downcast_ref::<Int64Array>()
        .ok_or(MicrostructureError::MalformedColumn {
            column: name,
            row: 0,
        })
}

fn column_decimal<'a>(
    batch: &'a RecordBatch,
    index: usize,
    name: &'static str,
) -> Result<&'a Decimal128Array, MicrostructureError> {
    batch
        .column(index)
        .as_any()
        .downcast_ref::<Decimal128Array>()
        .ok_or(MicrostructureError::MalformedColumn {
            column: name,
            row: 0,
        })
}

fn column_bool<'a>(
    batch: &'a RecordBatch,
    index: usize,
    name: &'static str,
) -> Result<&'a BooleanArray, MicrostructureError> {
    batch
        .column(index)
        .as_any()
        .downcast_ref::<BooleanArray>()
        .ok_or(MicrostructureError::MalformedColumn {
            column: name,
            row: 0,
        })
}

/// Diretório de um dia de dado de um `kind` (`"trades"`, `"book_ticker"`,
/// `"book_deltas"`, `"book_snapshots"`) para `symbol`.
pub fn day_dir(data_dir: &Path, symbol: &str, kind: &str, date: NaiveDate) -> PathBuf {
    data_dir.join(symbol).join(kind).join(date.to_string())
}

/// Caminho de um novo arquivo de flush dentro do dia — nome derivado do
/// instante do flush (`HHMMSS` + nanos), garantindo unicidade sem precisar
/// de um contador compartilhado entre chamadas.
fn flush_file_path(dir: &Path, at: DateTime<Utc>) -> PathBuf {
    dir.join(format!(
        "{:02}{:02}{:02}_{:09}.parquet",
        at.time().hour(),
        at.time().minute(),
        at.time().second(),
        at.timestamp_subsec_nanos()
    ))
}

fn write_batch(
    dir: &Path,
    at: DateTime<Utc>,
    schema: Arc<Schema>,
    batch: RecordBatch,
) -> Result<(), MicrostructureError> {
    fs::create_dir_all(dir).map_err(|source| MicrostructureError::Io {
        path: dir.display().to_string(),
        source,
    })?;
    let path = flush_file_path(dir, at);
    let file = File::create(&path).map_err(|source| MicrostructureError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let props = WriterProperties::builder()
        .set_compression(parquet::basic::Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props)).map_err(|source| {
        MicrostructureError::Parquet {
            path: path.display().to_string(),
            source,
        }
    })?;
    writer
        .write(&batch)
        .map_err(|source| MicrostructureError::Parquet {
            path: path.display().to_string(),
            source,
        })?;
    writer
        .close()
        .map_err(|source| MicrostructureError::Parquet {
            path: path.display().to_string(),
            source,
        })?;
    Ok(())
}

/// Nome do arquivo consolidado que `compact::compact_day` escreve. Quando
/// presente num diretório de partição, `read_all_batches` lê **só ele**,
/// ignorando qualquer outro `*.parquet` que ainda esteja lá — é o que
/// torna a compactação segura mesmo se a etapa de limpeza dos arquivos
/// originais for interrompida no meio: o arquivo novo já contém tudo, e um
/// arquivo antigo esquecido nunca é lido de novo (o que causaria
/// contagem duplicada), só fica como lixo cosmético até uma limpeza
/// manual ou uma nova compactação.
pub const COMPACTED_FILE_NAME: &str = "compacted.parquet";

fn read_all_batches(dir: &Path) -> Result<Vec<RecordBatch>, MicrostructureError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let compacted_path = dir.join(COMPACTED_FILE_NAME);
    let mut files: Vec<PathBuf> = if compacted_path.exists() {
        vec![compacted_path]
    } else {
        fs::read_dir(dir)
            .map_err(|source| MicrostructureError::Io {
                path: dir.display().to_string(),
                source,
            })?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "parquet"))
            .collect()
    };
    files.sort();

    let mut batches = Vec::new();
    for path in files {
        let file = File::open(&path).map_err(|source| MicrostructureError::Io {
            path: path.display().to_string(),
            source,
        })?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(file).map_err(|source| {
            MicrostructureError::Parquet {
                path: path.display().to_string(),
                source,
            }
        })?;
        let reader = builder
            .build()
            .map_err(|source| MicrostructureError::Parquet {
                path: path.display().to_string(),
                source,
            })?;
        for batch in reader {
            batches.push(batch?);
        }
    }
    Ok(batches)
}

use chrono::Timelike;

// ---------------------------------------------------------------------
// trades
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct TradeRow {
    pub exchange_trade_id: String,
    pub price: Decimal,
    pub quantity: Decimal,
    pub taker_side: Side,
    pub timestamp_ms: i64,
}

fn trades_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("exchange_trade_id", DataType::Utf8, false),
        decimal_field("price"),
        decimal_field("quantity"),
        Field::new("is_buy", DataType::Boolean, false),
        Field::new("timestamp_ms", DataType::Int64, false),
    ]))
}

pub fn write_trades(
    dir: &Path,
    at: DateTime<Utc>,
    rows: &[TradeRow],
) -> Result<(), MicrostructureError> {
    let mut ids = Vec::with_capacity(rows.len());
    let mut price = Vec::with_capacity(rows.len());
    let mut quantity = Vec::with_capacity(rows.len());
    let mut is_buy = Vec::with_capacity(rows.len());
    let mut ts = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        ids.push(row.exchange_trade_id.clone());
        price.push(decimal_to_scaled_mantissa(row.price, "price", i)?);
        quantity.push(decimal_to_scaled_mantissa(row.quantity, "quantity", i)?);
        is_buy.push(row.taker_side == Side::Buy);
        ts.push(row.timestamp_ms);
    }
    let schema = trades_schema();
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(arrow::array::StringArray::from(ids)),
            Arc::new(decimal_array(price)?),
            Arc::new(decimal_array(quantity)?),
            Arc::new(BooleanArray::from(is_buy)),
            Arc::new(Int64Array::from(ts)),
        ],
    )?;
    write_batch(dir, at, schema, batch)
}

pub fn read_trades_day(
    data_dir: &Path,
    symbol: &str,
    date: NaiveDate,
) -> Result<Vec<TradeRow>, MicrostructureError> {
    let dir = day_dir(data_dir, symbol, "trades", date);
    let mut out = Vec::new();
    for batch in read_all_batches(&dir)? {
        let ids = batch
            .column(0)
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .ok_or(MicrostructureError::MalformedColumn {
                column: "exchange_trade_id",
                row: 0,
            })?;
        let price = column_decimal(&batch, 1, "price")?;
        let quantity = column_decimal(&batch, 2, "quantity")?;
        let is_buy = column_bool(&batch, 3, "is_buy")?;
        let ts = column_i64(&batch, 4, "timestamp_ms")?;
        for row in 0..batch.num_rows() {
            out.push(TradeRow {
                exchange_trade_id: ids.value(row).to_string(),
                price: scaled_mantissa_to_decimal(price.value(row)),
                quantity: scaled_mantissa_to_decimal(quantity.value(row)),
                taker_side: if is_buy.value(row) {
                    Side::Buy
                } else {
                    Side::Sell
                },
                timestamp_ms: ts.value(row),
            });
        }
    }
    out.sort_by_key(|r| r.timestamp_ms);
    Ok(out)
}

// ---------------------------------------------------------------------
// book ticker (melhor bid/ask)
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct BookTickerRow {
    pub update_id: i64,
    pub bid_price: Decimal,
    pub bid_qty: Decimal,
    pub ask_price: Decimal,
    pub ask_qty: Decimal,
    pub timestamp_ms: i64,
}

fn book_ticker_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("update_id", DataType::Int64, false),
        decimal_field("bid_price"),
        decimal_field("bid_qty"),
        decimal_field("ask_price"),
        decimal_field("ask_qty"),
        Field::new("timestamp_ms", DataType::Int64, false),
    ]))
}

pub fn write_book_ticker(
    dir: &Path,
    at: DateTime<Utc>,
    rows: &[BookTickerRow],
) -> Result<(), MicrostructureError> {
    let mut update_id = Vec::with_capacity(rows.len());
    let mut bid_price = Vec::with_capacity(rows.len());
    let mut bid_qty = Vec::with_capacity(rows.len());
    let mut ask_price = Vec::with_capacity(rows.len());
    let mut ask_qty = Vec::with_capacity(rows.len());
    let mut ts = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        update_id.push(row.update_id);
        bid_price.push(decimal_to_scaled_mantissa(row.bid_price, "bid_price", i)?);
        bid_qty.push(decimal_to_scaled_mantissa(row.bid_qty, "bid_qty", i)?);
        ask_price.push(decimal_to_scaled_mantissa(row.ask_price, "ask_price", i)?);
        ask_qty.push(decimal_to_scaled_mantissa(row.ask_qty, "ask_qty", i)?);
        ts.push(row.timestamp_ms);
    }
    let schema = book_ticker_schema();
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(Int64Array::from(update_id)),
            Arc::new(decimal_array(bid_price)?),
            Arc::new(decimal_array(bid_qty)?),
            Arc::new(decimal_array(ask_price)?),
            Arc::new(decimal_array(ask_qty)?),
            Arc::new(Int64Array::from(ts)),
        ],
    )?;
    write_batch(dir, at, schema, batch)
}

pub fn read_book_ticker_day(
    data_dir: &Path,
    symbol: &str,
    date: NaiveDate,
) -> Result<Vec<BookTickerRow>, MicrostructureError> {
    let dir = day_dir(data_dir, symbol, "book_ticker", date);
    let mut out = Vec::new();
    for batch in read_all_batches(&dir)? {
        let update_id = column_i64(&batch, 0, "update_id")?;
        let bid_price = column_decimal(&batch, 1, "bid_price")?;
        let bid_qty = column_decimal(&batch, 2, "bid_qty")?;
        let ask_price = column_decimal(&batch, 3, "ask_price")?;
        let ask_qty = column_decimal(&batch, 4, "ask_qty")?;
        let ts = column_i64(&batch, 5, "timestamp_ms")?;
        for row in 0..batch.num_rows() {
            out.push(BookTickerRow {
                update_id: update_id.value(row),
                bid_price: scaled_mantissa_to_decimal(bid_price.value(row)),
                bid_qty: scaled_mantissa_to_decimal(bid_qty.value(row)),
                ask_price: scaled_mantissa_to_decimal(ask_price.value(row)),
                ask_qty: scaled_mantissa_to_decimal(ask_qty.value(row)),
                timestamp_ms: ts.value(row),
            });
        }
    }
    out.sort_by_key(|r| r.update_id);
    Ok(out)
}

// ---------------------------------------------------------------------
// book deltas (diff depth) — long format, uma linha por nível atualizado
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct BookDeltaRow {
    pub first_update_id: i64,
    pub final_update_id: i64,
    pub is_bid: bool,
    pub price: Decimal,
    pub quantity: Decimal,
    pub event_time_ms: i64,
}

fn book_deltas_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("first_update_id", DataType::Int64, false),
        Field::new("final_update_id", DataType::Int64, false),
        Field::new("is_bid", DataType::Boolean, false),
        decimal_field("price"),
        decimal_field("quantity"),
        Field::new("event_time_ms", DataType::Int64, false),
    ]))
}

pub fn write_book_deltas(
    dir: &Path,
    at: DateTime<Utc>,
    rows: &[BookDeltaRow],
) -> Result<(), MicrostructureError> {
    let mut first = Vec::with_capacity(rows.len());
    let mut last = Vec::with_capacity(rows.len());
    let mut is_bid = Vec::with_capacity(rows.len());
    let mut price = Vec::with_capacity(rows.len());
    let mut quantity = Vec::with_capacity(rows.len());
    let mut ts = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        first.push(row.first_update_id);
        last.push(row.final_update_id);
        is_bid.push(row.is_bid);
        price.push(decimal_to_scaled_mantissa(row.price, "price", i)?);
        quantity.push(decimal_to_scaled_mantissa(row.quantity, "quantity", i)?);
        ts.push(row.event_time_ms);
    }
    let schema = book_deltas_schema();
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(Int64Array::from(first)),
            Arc::new(Int64Array::from(last)),
            Arc::new(BooleanArray::from(is_bid)),
            Arc::new(decimal_array(price)?),
            Arc::new(decimal_array(quantity)?),
            Arc::new(Int64Array::from(ts)),
        ],
    )?;
    write_batch(dir, at, schema, batch)
}

pub fn read_book_deltas_day(
    data_dir: &Path,
    symbol: &str,
    date: NaiveDate,
) -> Result<Vec<BookDeltaRow>, MicrostructureError> {
    let dir = day_dir(data_dir, symbol, "book_deltas", date);
    let mut out = Vec::new();
    for batch in read_all_batches(&dir)? {
        let first = column_i64(&batch, 0, "first_update_id")?;
        let last = column_i64(&batch, 1, "final_update_id")?;
        let is_bid = column_bool(&batch, 2, "is_bid")?;
        let price = column_decimal(&batch, 3, "price")?;
        let quantity = column_decimal(&batch, 4, "quantity")?;
        let ts = column_i64(&batch, 5, "event_time_ms")?;
        for row in 0..batch.num_rows() {
            out.push(BookDeltaRow {
                first_update_id: first.value(row),
                final_update_id: last.value(row),
                is_bid: is_bid.value(row),
                price: scaled_mantissa_to_decimal(price.value(row)),
                quantity: scaled_mantissa_to_decimal(quantity.value(row)),
                event_time_ms: ts.value(row),
            });
        }
    }
    out.sort_by_key(|r| r.final_update_id);
    Ok(out)
}

/// Agrupa linhas "long format" de volta em `domain::OrderBookDelta`s — o
/// inverso de como `write_book_deltas` achata um evento com N níveis em N
/// linhas. Agrupa por `(first_update_id, final_update_id)`, preservando a
/// ordem de primeira aparição (que, vindo de `read_book_deltas_day`, já
/// está ordenada por `final_update_id`).
pub fn group_delta_rows(
    instrument_id: domain::InstrumentId,
    rows: &[BookDeltaRow],
) -> Result<Vec<domain::OrderBookDelta>, MicrostructureError> {
    let mut out: Vec<domain::OrderBookDelta> = Vec::new();
    for row in rows {
        let level = domain::BookLevel {
            price: row.price,
            quantity: row.quantity,
        };
        let needs_new_event = match out.last() {
            Some(last) => {
                last.first_update_id != row.first_update_id
                    || last.final_update_id != row.final_update_id
            }
            None => true,
        };
        if needs_new_event {
            out.push(domain::OrderBookDelta {
                instrument_id,
                first_update_id: row.first_update_id,
                final_update_id: row.final_update_id,
                bids: Vec::new(),
                asks: Vec::new(),
                timestamp: millis_to_utc(row.event_time_ms)?,
            });
        }
        let event = out.last_mut().expect("just pushed if needed");
        if row.is_bid {
            event.bids.push(level);
        } else {
            event.asks.push(level);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------
// book snapshots (REST) — long format, uma linha por nível
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct BookSnapshotRow {
    pub last_update_id: i64,
    pub is_bid: bool,
    pub level_index: i32,
    pub price: Decimal,
    pub quantity: Decimal,
    pub captured_at_ms: i64,
}

fn book_snapshots_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("last_update_id", DataType::Int64, false),
        Field::new("is_bid", DataType::Boolean, false),
        Field::new("level_index", DataType::Int32, false),
        decimal_field("price"),
        decimal_field("quantity"),
        Field::new("captured_at_ms", DataType::Int64, false),
    ]))
}

pub fn write_book_snapshot(
    dir: &Path,
    at: DateTime<Utc>,
    rows: &[BookSnapshotRow],
) -> Result<(), MicrostructureError> {
    let mut last_update_id = Vec::with_capacity(rows.len());
    let mut is_bid = Vec::with_capacity(rows.len());
    let mut level_index = Vec::with_capacity(rows.len());
    let mut price = Vec::with_capacity(rows.len());
    let mut quantity = Vec::with_capacity(rows.len());
    let mut ts = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        last_update_id.push(row.last_update_id);
        is_bid.push(row.is_bid);
        level_index.push(row.level_index);
        price.push(decimal_to_scaled_mantissa(row.price, "price", i)?);
        quantity.push(decimal_to_scaled_mantissa(row.quantity, "quantity", i)?);
        ts.push(row.captured_at_ms);
    }
    let schema = book_snapshots_schema();
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(Int64Array::from(last_update_id)),
            Arc::new(BooleanArray::from(is_bid)),
            Arc::new(arrow::array::Int32Array::from(level_index)),
            Arc::new(decimal_array(price)?),
            Arc::new(decimal_array(quantity)?),
            Arc::new(Int64Array::from(ts)),
        ],
    )?;
    write_batch(dir, at, schema, batch)
}

pub fn read_book_snapshots_day(
    data_dir: &Path,
    symbol: &str,
    date: NaiveDate,
) -> Result<Vec<BookSnapshotRow>, MicrostructureError> {
    let dir = day_dir(data_dir, symbol, "book_snapshots", date);
    let mut out = Vec::new();
    for batch in read_all_batches(&dir)? {
        let last_update_id = column_i64(&batch, 0, "last_update_id")?;
        let is_bid = column_bool(&batch, 1, "is_bid")?;
        let level_index = batch
            .column(2)
            .as_any()
            .downcast_ref::<arrow::array::Int32Array>()
            .ok_or(MicrostructureError::MalformedColumn {
                column: "level_index",
                row: 0,
            })?;
        let price = column_decimal(&batch, 3, "price")?;
        let quantity = column_decimal(&batch, 4, "quantity")?;
        let ts = column_i64(&batch, 5, "captured_at_ms")?;
        for row in 0..batch.num_rows() {
            out.push(BookSnapshotRow {
                last_update_id: last_update_id.value(row),
                is_bid: is_bid.value(row),
                level_index: level_index.value(row),
                price: scaled_mantissa_to_decimal(price.value(row)),
                quantity: scaled_mantissa_to_decimal(quantity.value(row)),
                captured_at_ms: ts.value(row),
            });
        }
    }
    out.sort_by_key(|r| (r.last_update_id, !r.is_bid, r.level_index));
    Ok(out)
}

/// Reconstrói um `book::DepthSnapshot` a partir das linhas "long format" de
/// um único snapshot (todas com o mesmo `last_update_id`) — inverso de como
/// `write_book_snapshot` achata os níveis.
pub fn rows_to_depth_snapshot(
    instrument_id: domain::InstrumentId,
    rows: &[BookSnapshotRow],
) -> Result<Option<crate::book::DepthSnapshot>, MicrostructureError> {
    let Some(first) = rows.first() else {
        return Ok(None);
    };
    let mut bids: Vec<(i32, domain::BookLevel)> = Vec::new();
    let mut asks: Vec<(i32, domain::BookLevel)> = Vec::new();
    for row in rows {
        let level = domain::BookLevel {
            price: row.price,
            quantity: row.quantity,
        };
        if row.is_bid {
            bids.push((row.level_index, level));
        } else {
            asks.push((row.level_index, level));
        }
    }
    bids.sort_by_key(|(idx, _)| *idx);
    asks.sort_by_key(|(idx, _)| *idx);
    Ok(Some(crate::book::DepthSnapshot {
        instrument_id,
        last_update_id: first.last_update_id,
        bids: bids.into_iter().map(|(_, l)| l).collect(),
        asks: asks.into_iter().map(|(_, l)| l).collect(),
        captured_at: millis_to_utc(first.captured_at_ms)?,
    }))
}

// ---------------------------------------------------------------------
// grade de features (1s por default) — ver `grid::snapshot_grid`
// ---------------------------------------------------------------------

fn nullable_f64_field(name: &str) -> Field {
    Field::new(name, DataType::Float64, true)
}

fn column_f64<'a>(
    batch: &'a RecordBatch,
    index: usize,
    name: &'static str,
) -> Result<&'a Float64Array, MicrostructureError> {
    batch
        .column(index)
        .as_any()
        .downcast_ref::<Float64Array>()
        .ok_or(MicrostructureError::MalformedColumn {
            column: name,
            row: 0,
        })
}

fn opt_f64(array: &Float64Array, row: usize) -> Option<f64> {
    if array.is_null(row) {
        None
    } else {
        Some(array.value(row))
    }
}

/// Uma linha da grade de features de microestrutura — os mesmos 10 campos
/// de `snapshot::MicrostructureSnapshot` (menos `instrument_id`, que já
/// está no caminho do arquivo), amostrados em pontos de grade regulares
/// por `grid::snapshot_grid`. Campos `None` (janela ainda sem dado
/// suficiente) são gravados como `NULL` Parquet, não um zero inventado.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureGridRow {
    pub timestamp_ms: i64,
    pub spread_abs: Option<f64>,
    pub spread_pct: Option<f64>,
    pub mid_price: Option<f64>,
    pub microprice: Option<f64>,
    pub bid_ask_imbalance: Option<f64>,
    pub book_imbalance: Option<f64>,
    pub trade_imbalance: Option<f64>,
    pub volume_delta: Option<f64>,
    pub trade_intensity: Option<f64>,
    pub order_flow_imbalance: Option<f64>,
}

impl FeatureGridRow {
    pub fn from_snapshot(s: &crate::snapshot::MicrostructureSnapshot) -> Self {
        Self {
            timestamp_ms: s.timestamp.timestamp_millis(),
            spread_abs: s.spread_abs,
            spread_pct: s.spread_pct,
            mid_price: s.mid_price,
            microprice: s.microprice,
            bid_ask_imbalance: s.bid_ask_imbalance,
            book_imbalance: s.book_imbalance,
            trade_imbalance: s.trade_imbalance,
            volume_delta: s.volume_delta,
            trade_intensity: s.trade_intensity,
            order_flow_imbalance: s.order_flow_imbalance,
        }
    }
}

fn feature_grid_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("timestamp_ms", DataType::Int64, false),
        nullable_f64_field("spread_abs"),
        nullable_f64_field("spread_pct"),
        nullable_f64_field("mid_price"),
        nullable_f64_field("microprice"),
        nullable_f64_field("bid_ask_imbalance"),
        nullable_f64_field("book_imbalance"),
        nullable_f64_field("trade_imbalance"),
        nullable_f64_field("volume_delta"),
        nullable_f64_field("trade_intensity"),
        nullable_f64_field("order_flow_imbalance"),
    ]))
}

pub fn write_feature_grid(
    dir: &Path,
    at: DateTime<Utc>,
    rows: &[FeatureGridRow],
) -> Result<(), MicrostructureError> {
    let ts: Vec<i64> = rows.iter().map(|r| r.timestamp_ms).collect();
    macro_rules! col {
        ($field:ident) => {
            Float64Array::from(rows.iter().map(|r| r.$field).collect::<Vec<Option<f64>>>())
        };
    }
    let schema = feature_grid_schema();
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(Int64Array::from(ts)),
            Arc::new(col!(spread_abs)),
            Arc::new(col!(spread_pct)),
            Arc::new(col!(mid_price)),
            Arc::new(col!(microprice)),
            Arc::new(col!(bid_ask_imbalance)),
            Arc::new(col!(book_imbalance)),
            Arc::new(col!(trade_imbalance)),
            Arc::new(col!(volume_delta)),
            Arc::new(col!(trade_intensity)),
            Arc::new(col!(order_flow_imbalance)),
        ],
    )?;
    write_batch(dir, at, schema, batch)
}

pub fn read_feature_grid_day(
    data_dir: &Path,
    symbol: &str,
    date: NaiveDate,
) -> Result<Vec<FeatureGridRow>, MicrostructureError> {
    let dir = day_dir(data_dir, symbol, "feature_grid", date);
    let mut out = Vec::new();
    for batch in read_all_batches(&dir)? {
        let ts = column_i64(&batch, 0, "timestamp_ms")?;
        let spread_abs = column_f64(&batch, 1, "spread_abs")?;
        let spread_pct = column_f64(&batch, 2, "spread_pct")?;
        let mid_price = column_f64(&batch, 3, "mid_price")?;
        let microprice = column_f64(&batch, 4, "microprice")?;
        let bid_ask_imbalance = column_f64(&batch, 5, "bid_ask_imbalance")?;
        let book_imbalance = column_f64(&batch, 6, "book_imbalance")?;
        let trade_imbalance = column_f64(&batch, 7, "trade_imbalance")?;
        let volume_delta = column_f64(&batch, 8, "volume_delta")?;
        let trade_intensity = column_f64(&batch, 9, "trade_intensity")?;
        let order_flow_imbalance = column_f64(&batch, 10, "order_flow_imbalance")?;

        for row in 0..batch.num_rows() {
            out.push(FeatureGridRow {
                timestamp_ms: ts.value(row),
                spread_abs: opt_f64(spread_abs, row),
                spread_pct: opt_f64(spread_pct, row),
                mid_price: opt_f64(mid_price, row),
                microprice: opt_f64(microprice, row),
                bid_ask_imbalance: opt_f64(bid_ask_imbalance, row),
                book_imbalance: opt_f64(book_imbalance, row),
                trade_imbalance: opt_f64(trade_imbalance, row),
                volume_delta: opt_f64(volume_delta, row),
                trade_intensity: opt_f64(trade_intensity, row),
                order_flow_imbalance: opt_f64(order_flow_imbalance, row),
            });
        }
    }
    out.sort_by_key(|r| r.timestamp_ms);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn temp_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "microstructure-store-test-{label}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn trades_round_trip_exactly_including_a_real_btc_decimal() {
        let base = temp_dir("trades");
        let symbol = "TESTUSDT";
        let now = Utc::now();
        let dir = day_dir(&base, symbol, "trades", now.date_naive());

        let rows = vec![
            TradeRow {
                exchange_trade_id: "12345".to_string(),
                price: dec!(50000.12345678),
                quantity: dec!(4244.592940), // o mesmo valor que já quebrou f64 em outra fase do projeto
                taker_side: Side::Buy,
                timestamp_ms: 1_700_000_000_000,
            },
            TradeRow {
                exchange_trade_id: "12346".to_string(),
                price: dec!(49999.00000001),
                quantity: dec!(0.00010000),
                taker_side: Side::Sell,
                timestamp_ms: 1_700_000_000_500,
            },
        ];

        write_trades(&dir, now, &rows).unwrap();
        let restored = read_trades_day(&base, symbol, now.date_naive()).unwrap();

        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0], rows[0]);
        assert_eq!(restored[1], rows[1]);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn book_ticker_round_trips_and_reads_in_update_id_order() {
        let base = temp_dir("ticker");
        let symbol = "TESTUSDT";
        let date = Utc::now().date_naive();
        let dir = day_dir(&base, symbol, "book_ticker", date);

        let row_a = BookTickerRow {
            update_id: 200,
            bid_price: dec!(100),
            bid_qty: dec!(1),
            ask_price: dec!(101),
            ask_qty: dec!(2),
            timestamp_ms: 2000,
        };
        let row_b = BookTickerRow {
            update_id: 100,
            bid_price: dec!(99),
            bid_qty: dec!(3),
            ask_price: dec!(102),
            ask_qty: dec!(4),
            timestamp_ms: 1000,
        };
        // Grava fora de ordem, em dois flushes distintos.
        write_book_ticker(&dir, Utc::now(), std::slice::from_ref(&row_a)).unwrap();
        write_book_ticker(&dir, Utc::now(), std::slice::from_ref(&row_b)).unwrap();

        let restored = read_book_ticker_day(&base, symbol, date).unwrap();
        assert_eq!(restored, vec![row_b, row_a]);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn book_deltas_round_trip_and_regroup_into_the_original_events() {
        let base = temp_dir("deltas");
        let symbol = "TESTUSDT";
        let date = Utc::now().date_naive();
        let dir = day_dir(&base, symbol, "book_deltas", date);
        let instrument_id = domain::InstrumentId::new();

        let event_time_ms = Utc::now().timestamp_millis();
        let rows = vec![
            BookDeltaRow {
                first_update_id: 10,
                final_update_id: 12,
                is_bid: true,
                price: dec!(100),
                quantity: dec!(5),
                event_time_ms,
            },
            BookDeltaRow {
                first_update_id: 10,
                final_update_id: 12,
                is_bid: false,
                price: dec!(101),
                quantity: dec!(3),
                event_time_ms,
            },
            BookDeltaRow {
                first_update_id: 13,
                final_update_id: 13,
                is_bid: true,
                price: dec!(99.5),
                quantity: dec!(1),
                event_time_ms: event_time_ms + 100,
            },
        ];
        write_book_deltas(&dir, Utc::now(), &rows).unwrap();

        let restored = read_book_deltas_day(&base, symbol, date).unwrap();
        assert_eq!(restored.len(), 3);

        let events = group_delta_rows(instrument_id, &restored).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].first_update_id, 10);
        assert_eq!(events[0].final_update_id, 12);
        assert_eq!(
            events[0].bids,
            vec![domain::BookLevel {
                price: dec!(100),
                quantity: dec!(5)
            }]
        );
        assert_eq!(
            events[0].asks,
            vec![domain::BookLevel {
                price: dec!(101),
                quantity: dec!(3)
            }]
        );
        assert_eq!(events[1].first_update_id, 13);
        assert_eq!(
            events[1].bids,
            vec![domain::BookLevel {
                price: dec!(99.5),
                quantity: dec!(1)
            }]
        );

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn book_snapshot_round_trips_and_rebuilds_depth_snapshot() {
        let base = temp_dir("snapshot");
        let symbol = "TESTUSDT";
        let date = Utc::now().date_naive();
        let dir = day_dir(&base, symbol, "book_snapshots", date);
        let instrument_id = domain::InstrumentId::new();
        let captured_at_ms = Utc::now().timestamp_millis();

        let rows = vec![
            BookSnapshotRow {
                last_update_id: 500,
                is_bid: true,
                level_index: 1,
                price: dec!(99),
                quantity: dec!(2),
                captured_at_ms,
            },
            BookSnapshotRow {
                last_update_id: 500,
                is_bid: true,
                level_index: 0,
                price: dec!(100),
                quantity: dec!(1),
                captured_at_ms,
            },
            BookSnapshotRow {
                last_update_id: 500,
                is_bid: false,
                level_index: 0,
                price: dec!(101),
                quantity: dec!(3),
                captured_at_ms,
            },
        ];
        write_book_snapshot(&dir, Utc::now(), &rows).unwrap();

        let restored = read_book_snapshots_day(&base, symbol, date).unwrap();
        let snapshot = rows_to_depth_snapshot(instrument_id, &restored)
            .unwrap()
            .unwrap();

        assert_eq!(snapshot.last_update_id, 500);
        assert_eq!(
            snapshot.bids,
            vec![
                domain::BookLevel {
                    price: dec!(100),
                    quantity: dec!(1)
                },
                domain::BookLevel {
                    price: dec!(99),
                    quantity: dec!(2)
                },
            ]
        );
        assert_eq!(
            snapshot.asks,
            vec![domain::BookLevel {
                price: dec!(101),
                quantity: dec!(3)
            }]
        );

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn reading_a_missing_day_returns_an_empty_vec_not_an_error() {
        let base = temp_dir("missing");
        let restored = read_trades_day(&base, "NOPE", Utc::now().date_naive()).unwrap();
        assert!(restored.is_empty());
    }

    #[test]
    fn rejects_a_value_with_more_decimal_places_than_the_fixed_column_scale() {
        let base = temp_dir("scale");
        let symbol = "TESTUSDT";
        let date = Utc::now().date_naive();
        let dir = day_dir(&base, symbol, "trades", date);

        let mut bad = TradeRow {
            exchange_trade_id: "1".to_string(),
            price: dec!(100),
            quantity: dec!(1),
            taker_side: Side::Buy,
            timestamp_ms: 0,
        };
        bad.quantity = Decimal::new(123456789, 9); // 9 casas decimais > DECIMAL_SCALE

        let result = write_trades(&dir, Utc::now(), &[bad]);
        assert!(matches!(
            result,
            Err(MicrostructureError::ScaleTooLarge {
                column: "quantity",
                ..
            })
        ));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn feature_grid_round_trips_including_null_fields() {
        let base = temp_dir("grid");
        let symbol = "TESTUSDT";
        let date = Utc::now().date_naive();
        let dir = day_dir(&base, symbol, "feature_grid", date);

        let rows = vec![
            FeatureGridRow {
                timestamp_ms: 1_000,
                spread_abs: Some(0.01),
                spread_pct: Some(0.0001),
                mid_price: Some(100.0),
                microprice: Some(100.02),
                bid_ask_imbalance: Some(0.2),
                book_imbalance: Some(-0.1),
                // Ainda sem trade na janela -> None, não zero.
                trade_imbalance: None,
                volume_delta: None,
                trade_intensity: None,
                order_flow_imbalance: None,
            },
            FeatureGridRow {
                timestamp_ms: 2_000,
                spread_abs: Some(0.02),
                spread_pct: Some(0.0002),
                mid_price: Some(100.5),
                microprice: Some(100.48),
                bid_ask_imbalance: Some(-0.3),
                book_imbalance: Some(0.15),
                trade_imbalance: Some(0.5),
                volume_delta: Some(-2.5),
                trade_intensity: Some(1.2),
                order_flow_imbalance: Some(-10.0),
            },
        ];
        write_feature_grid(&dir, Utc::now(), &rows).unwrap();

        let restored = read_feature_grid_day(&base, symbol, date).unwrap();
        assert_eq!(restored, rows);
        assert_eq!(restored[0].trade_imbalance, None);
        assert_eq!(restored[1].trade_imbalance, Some(0.5));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn feature_grid_row_from_snapshot_maps_every_field() {
        use crate::snapshot::MicrostructureSnapshot;

        let snapshot = MicrostructureSnapshot {
            instrument_id: domain::InstrumentId::new(),
            timestamp: Utc::now(),
            spread_abs: Some(1.0),
            spread_pct: Some(2.0),
            mid_price: Some(3.0),
            microprice: Some(4.0),
            bid_ask_imbalance: Some(5.0),
            book_imbalance: Some(6.0),
            trade_imbalance: Some(7.0),
            volume_delta: Some(8.0),
            trade_intensity: Some(9.0),
            order_flow_imbalance: Some(10.0),
        };
        let row = FeatureGridRow::from_snapshot(&snapshot);
        assert_eq!(row.timestamp_ms, snapshot.timestamp.timestamp_millis());
        assert_eq!(row.spread_abs, Some(1.0));
        assert_eq!(row.order_flow_imbalance, Some(10.0));
    }
}
