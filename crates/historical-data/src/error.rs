use thiserror::Error;

#[derive(Debug, Error)]
pub enum HistoricalDataError {
    #[error("market data request failed: {0}")]
    MarketData(#[from] market_data::MarketDataError),
    #[error("parquet I/O failed for {path}: {source}")]
    Parquet {
        path: String,
        #[source]
        source: parquet::errors::ParquetError,
    },
    #[error("arrow error while (de)serializing candles: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
    #[error("filesystem error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("stored candle column {column} had an unexpected type or was null at row {row}")]
    MalformedColumn { column: &'static str, row: usize },
    /// Preço/volume são gravados como `Decimal128` de escala fixa (ver o
    /// doc de `store`) — um valor com mais casas decimais do que a coluna
    /// comporta não pode ser arredondado silenciosamente para caber; a
    /// gravação falha alto em vez disso. Não deveria acontecer com dados
    /// reais da Binance (que nunca emite mais que 8 casas decimais), mas é
    /// uma checagem explícita, não uma suposição.
    #[error(
        "candle column {column} row {row} has value {value} with more decimal places than the \
         fixed column scale of {max_scale} supports"
    )]
    ScaleTooLarge {
        column: &'static str,
        row: usize,
        value: String,
        max_scale: u32,
    },
}
