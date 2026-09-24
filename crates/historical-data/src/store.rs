//! Persistência local de candles em Parquet: um arquivo por
//! instrumento+timeframe (`{data_dir}/{symbol}/{timeframe}.parquet`).
//!
//! **Preço e volume são gravados como `Decimal128` (inteiro escalado),
//! nunca `Float64`.** `f64` é apropriado para indicadores derivados
//! (`features`/`analytics` já usam `f64` para regressão, RSI, etc. — ali
//! um erro de arredondamento de ponto flutuante não corrompe contabilidade
//! financeira), mas não para o caminho financeiro: candles alimentam
//! `BacktestRunner`/`RiskEngine`/`PaperBroker`/`PortfolioManager`
//! diretamente, e nenhum ponto flutuante binário representa todo `Decimal`
//! base-10 exatamente — igualdade após round-trip `Decimal -> f64 ->
//! Decimal` (a abordagem anterior deste módulo) é uma checagem empírica,
//! não uma garantia; um valor real da Binance (`4244.592940`) já expôs uma
//! rota de reconstrução que não fechava esse round-trip.
//!
//! `Decimal128(DECIMAL_PRECISION, DECIMAL_SCALE)` é exato por construção:
//! grava o mantissa inteiro (`Decimal::mantissa()`) de cada valor já
//! reescalado para `DECIMAL_SCALE` casas decimais — puro deslocamento de
//! base-10, nunca uma conversão de base (2 vs. 10) que poderia perder
//! informação. `DECIMAL_SCALE = 8` casas decimais cobre com folga qualquer
//! valor que a própria Binance emite (nenhum campo OHLCV do exchange tem
//! mais que 8 casas decimais); um valor que *precisasse* de mais falha alto
//! (`HistoricalDataError::ScaleTooLarge`) em vez de ser silenciosamente
//! arredondado.
//!
//! `instrument_id`/`timeframe` não são colunas: o arquivo já os codifica
//! no caminho, e só candles fechados (`is_closed`) são gravados — a barra
//! em formação nunca é persistida.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::array::{Decimal128Array, Int64Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use domain::{Candle, InstrumentId, Timeframe};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use rust_decimal::Decimal;

use crate::error::HistoricalDataError;

/// Casas decimais fixas de todas as colunas de preço/volume — bem acima
/// das no máximo 8 que a Binance já emite hoje (ver o doc do módulo).
const DECIMAL_SCALE: u32 = 8;
/// Dígitos totais (parte inteira + `DECIMAL_SCALE`) — `Decimal128` permite
/// até 38; folga generosa para qualquer preço/volume realista de cripto.
const DECIMAL_PRECISION: u8 = 38;

fn decimal_field(name: &str) -> Field {
    Field::new(
        name,
        DataType::Decimal128(DECIMAL_PRECISION, DECIMAL_SCALE as i8),
        false,
    )
}

fn schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("open_time_ms", DataType::Int64, false),
        Field::new("close_time_ms", DataType::Int64, false),
        decimal_field("open"),
        decimal_field("high"),
        decimal_field("low"),
        decimal_field("close"),
        decimal_field("volume"),
    ]))
}

/// Caminho do arquivo Parquet para `symbol`/`timeframe` dentro de
/// `data_dir` — a única forma de nomear esses arquivos, para que
/// download/leitura nunca divirjam sobre onde procurar.
pub fn candle_file_path(data_dir: &Path, symbol: &str, timeframe: Timeframe) -> PathBuf {
    data_dir.join(symbol).join(format!("{}.parquet", timeframe))
}

/// Reescala `value` para exatamente `DECIMAL_SCALE` casas decimais e
/// devolve o mantissa inteiro resultante — exato por construção: como só
/// aumentamos a escala (nunca diminuímos, ver a checagem abaixo), a
/// operação é puro deslocamento de dígitos em base 10, nunca um
/// arredondamento. Erra alto se `value` já tiver mais casas decimais que
/// `DECIMAL_SCALE` comporta, em vez de arredondar silenciosamente.
fn decimal_to_scaled_mantissa(
    value: Decimal,
    column: &'static str,
    row: usize,
) -> Result<i128, HistoricalDataError> {
    if value.scale() > DECIMAL_SCALE {
        return Err(HistoricalDataError::ScaleTooLarge {
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

/// Inverso exato de [`decimal_to_scaled_mantissa`] — reconstrói o
/// `Decimal` original a partir do mantissa inteiro e da escala fixa da
/// coluna. Uma construção algébrica direta (`mantissa * 10^-DECIMAL_SCALE`),
/// não uma busca/aproximação — não há como divergir do valor gravado.
fn scaled_mantissa_to_decimal(mantissa: i128) -> Decimal {
    Decimal::from_i128_with_scale(mantissa, DECIMAL_SCALE)
}

/// Grava `candles` (devem já estar ordenados por `open_time` — este módulo
/// não reordena) em `path`, criando o diretório pai se necessário e
/// sobrescrevendo qualquer arquivo existente por inteiro (Parquet não é
/// append-friendly; reescrever é simples e barato na escala de dados deste
/// projeto — ver `update::update_symbol`).
pub fn write_candles(path: &Path, candles: &[Candle]) -> Result<(), HistoricalDataError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| HistoricalDataError::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }

    let mut open_time_ms = Vec::with_capacity(candles.len());
    let mut close_time_ms = Vec::with_capacity(candles.len());
    let mut open = Vec::with_capacity(candles.len());
    let mut high = Vec::with_capacity(candles.len());
    let mut low = Vec::with_capacity(candles.len());
    let mut close = Vec::with_capacity(candles.len());
    let mut volume = Vec::with_capacity(candles.len());

    for (row, candle) in candles.iter().enumerate() {
        open_time_ms.push(candle.open_time.timestamp_millis());
        close_time_ms.push(candle.close_time.timestamp_millis());
        open.push(decimal_to_scaled_mantissa(candle.open, "open", row)?);
        high.push(decimal_to_scaled_mantissa(candle.high, "high", row)?);
        low.push(decimal_to_scaled_mantissa(candle.low, "low", row)?);
        close.push(decimal_to_scaled_mantissa(candle.close, "close", row)?);
        volume.push(decimal_to_scaled_mantissa(candle.volume, "volume", row)?);
    }

    let decimal_array = |values: Vec<i128>| -> Result<Decimal128Array, HistoricalDataError> {
        Decimal128Array::from(values)
            .with_precision_and_scale(DECIMAL_PRECISION, DECIMAL_SCALE as i8)
            .map_err(HistoricalDataError::from)
    };

    let schema = schema();
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(Int64Array::from(open_time_ms)),
            Arc::new(Int64Array::from(close_time_ms)),
            Arc::new(decimal_array(open)?),
            Arc::new(decimal_array(high)?),
            Arc::new(decimal_array(low)?),
            Arc::new(decimal_array(close)?),
            Arc::new(decimal_array(volume)?),
        ],
    )?;

    let file = File::create(path).map_err(|source| HistoricalDataError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let props = WriterProperties::builder()
        .set_compression(parquet::basic::Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props)).map_err(|source| {
        HistoricalDataError::Parquet {
            path: path.display().to_string(),
            source,
        }
    })?;
    writer
        .write(&batch)
        .map_err(|source| HistoricalDataError::Parquet {
            path: path.display().to_string(),
            source,
        })?;
    writer
        .close()
        .map_err(|source| HistoricalDataError::Parquet {
            path: path.display().to_string(),
            source,
        })?;
    Ok(())
}

/// Lê `path` de volta em `Candle`s (ordenados como gravados), reconstruindo
/// `instrument_id`/`timeframe`/`is_closed` (sempre `true` — só candles
/// fechados são gravados) a partir do que o arquivo *não* guarda. Retorna
/// `Ok(None)` se `path` não existir — não é um erro, é "ainda não há
/// histórico local para este símbolo/timeframe".
pub fn read_candles(
    path: &Path,
    instrument_id: InstrumentId,
    timeframe: Timeframe,
) -> Result<Option<Vec<Candle>>, HistoricalDataError> {
    if !path.exists() {
        return Ok(None);
    }

    let file = File::open(path).map_err(|source| HistoricalDataError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file).map_err(|source| {
        HistoricalDataError::Parquet {
            path: path.display().to_string(),
            source,
        }
    })?;
    let reader = builder
        .build()
        .map_err(|source| HistoricalDataError::Parquet {
            path: path.display().to_string(),
            source,
        })?;

    let mut candles = Vec::new();
    for batch in reader {
        let batch = batch?;
        let open_time_ms = column_i64(&batch, 0, "open_time_ms")?;
        let close_time_ms = column_i64(&batch, 1, "close_time_ms")?;
        let open = column_decimal(&batch, 2, "open")?;
        let high = column_decimal(&batch, 3, "high")?;
        let low = column_decimal(&batch, 4, "low")?;
        let close = column_decimal(&batch, 5, "close")?;
        let volume = column_decimal(&batch, 6, "volume")?;

        for row in 0..batch.num_rows() {
            candles.push(Candle {
                instrument_id,
                timeframe,
                open_time: millis_to_utc(open_time_ms.value(row))?,
                close_time: millis_to_utc(close_time_ms.value(row))?,
                open: scaled_mantissa_to_decimal(open.value(row)),
                high: scaled_mantissa_to_decimal(high.value(row)),
                low: scaled_mantissa_to_decimal(low.value(row)),
                close: scaled_mantissa_to_decimal(close.value(row)),
                volume: scaled_mantissa_to_decimal(volume.value(row)),
                is_closed: true,
            });
        }
    }
    Ok(Some(candles))
}

fn column_i64<'a>(
    batch: &'a RecordBatch,
    index: usize,
    name: &'static str,
) -> Result<&'a Int64Array, HistoricalDataError> {
    batch
        .column(index)
        .as_any()
        .downcast_ref::<Int64Array>()
        .ok_or(HistoricalDataError::MalformedColumn {
            column: name,
            row: 0,
        })
}

fn column_decimal<'a>(
    batch: &'a RecordBatch,
    index: usize,
    name: &'static str,
) -> Result<&'a Decimal128Array, HistoricalDataError> {
    batch
        .column(index)
        .as_any()
        .downcast_ref::<Decimal128Array>()
        .ok_or(HistoricalDataError::MalformedColumn {
            column: name,
            row: 0,
        })
}

fn millis_to_utc(ms: i64) -> Result<chrono::DateTime<chrono::Utc>, HistoricalDataError> {
    use chrono::TimeZone;
    chrono::Utc
        .timestamp_millis_opt(ms)
        .single()
        .ok_or(HistoricalDataError::MalformedColumn {
            column: "*_time_ms",
            row: 0,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use rust_decimal_macros::dec;

    fn candle(minute: i64, open: Decimal, close: Decimal) -> Candle {
        let open_time =
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap() + chrono::Duration::minutes(minute);
        Candle {
            instrument_id: InstrumentId::new(),
            timeframe: Timeframe::M1,
            open_time,
            close_time: open_time + chrono::Duration::minutes(1),
            open,
            high: open.max(close) + dec!(1),
            low: open.min(close) - dec!(1),
            close,
            volume: dec!(1234.5678),
            is_closed: true,
        }
    }

    #[test]
    fn regression_real_binance_volume_that_broke_the_old_f64_scheme_round_trips_exactly() {
        // O valor real que expôs o bug do esquema anterior baseado em
        // `f64` (`Decimal::from_f64` reconstruía 4244.592940000001, não
        // 4244.592940) — com `Decimal128`, é só um deslocamento de dígitos
        // em base 10, exato por construção, sem depender de nenhuma
        // aproximação de ponto flutuante.
        let value = dec!(4244.592940);
        let mantissa = decimal_to_scaled_mantissa(value, "volume", 0).unwrap();
        let restored = scaled_mantissa_to_decimal(mantissa);
        assert_eq!(restored, value);
    }

    #[test]
    fn rejects_a_value_with_more_decimal_places_than_the_fixed_column_scale() {
        // 9 casas decimais > DECIMAL_SCALE (8) — a Binance nunca emite
        // isso hoje, mas se algum dia emitisse, a gravação deve falhar
        // alto em vez de arredondar silenciosamente para 8 casas.
        let dir = std::env::temp_dir().join(format!("historical-data-test-{}", uuid_like()));
        let path = dir.join("BTCUSDT").join("1m.parquet");
        let mut bad = candle(0, dec!(100), dec!(101));
        bad.volume = Decimal::new(123456789, 9); // 0.123456789 -> 9 casas decimais

        let result = write_candles(&path, &[bad]);
        assert!(
            matches!(
                result,
                Err(HistoricalDataError::ScaleTooLarge {
                    column: "volume",
                    ..
                })
            ),
            "expected a ScaleTooLarge error, got {result:?}"
        );
        assert!(
            !path.exists(),
            "a rejected write must not leave a partial/corrupted file behind"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn round_trips_realistic_crypto_prices_exactly() {
        let dir = std::env::temp_dir().join(format!("historical-data-test-{}", uuid_like()));
        let path = dir.join("BTCUSDT").join("1m.parquet");

        let mut candles = vec![
            candle(0, dec!(50000.12345678), dec!(50010.87654321)),
            candle(1, dec!(50010.87654321), dec!(49999.00000001)),
            candle(2, dec!(0.01575800), dec!(0.01634790)),
        ];
        candles[0].volume = dec!(4244.592940);

        write_candles(&path, &candles).unwrap();
        let instrument_id = candles[0].instrument_id;
        let read_back = read_candles(&path, instrument_id, Timeframe::M1)
            .unwrap()
            .expect("file was just written, must exist");

        assert_eq!(read_back.len(), candles.len());
        for (original, restored) in candles.iter().zip(read_back.iter()) {
            assert_eq!(restored.open_time, original.open_time);
            assert_eq!(restored.close_time, original.close_time);
            assert_eq!(restored.open, original.open);
            assert_eq!(restored.high, original.high);
            assert_eq!(restored.low, original.low);
            assert_eq!(restored.close, original.close);
            assert_eq!(restored.volume, original.volume);
            assert!(restored.is_closed);
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_candles_returns_none_when_file_is_missing() {
        let path = std::env::temp_dir()
            .join("historical-data-test-missing")
            .join("BTCUSDT")
            .join("1m.parquet");
        let result = read_candles(&path, InstrumentId::new(), Timeframe::M1).unwrap();
        assert!(result.is_none());
    }

    fn uuid_like() -> String {
        format!(
            "{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }
}
