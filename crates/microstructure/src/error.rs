use thiserror::Error;

#[derive(Debug, Error)]
pub enum MicrostructureError {
    #[error("http request to {url} failed: {source}")]
    Http {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("exchange returned an error response: {0}")]
    ExchangeError(String),
    #[error("failed to parse exchange payload: {0}")]
    Parse(String),
    #[error("parquet I/O failed for {path}: {source}")]
    Parquet {
        path: String,
        #[source]
        source: parquet::errors::ParquetError,
    },
    #[error("arrow error while (de)serializing microstructure data: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
    #[error("filesystem error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("stored column {column} had an unexpected type or was null at row {row}")]
    MalformedColumn { column: &'static str, row: usize },
    /// Mesma garantia de `historical-data::store`: preço/quantidade são
    /// gravados como `Decimal128` de escala fixa, nunca `f64`. Um valor com
    /// mais casas decimais do que a coluna comporta falha alto em vez de
    /// arredondar silenciosamente.
    #[error(
        "column {column} row {row} has value {value} with more decimal places than the fixed \
         column scale of {max_scale} supports"
    )]
    ScaleTooLarge {
        column: &'static str,
        row: usize,
        value: String,
        max_scale: u32,
    },
    /// Dois eventos de diff consecutivos do book não se encaixam
    /// (`first_update_id` do novo evento != `final_update_id` do evento
    /// anterior + 1) — o livro local não pode mais ser confiado até
    /// re-semear a partir de um snapshot novo. Ver `book::LocalOrderBook`.
    #[error(
        "order book sequence gap: expected next first_update_id to be {expected}, got {got} \
         (instrument may be desynced, needs a fresh snapshot)"
    )]
    SequenceGap { expected: i64, got: i64 },
    /// O livro local recebeu um delta antes de ser semeado com um snapshot
    /// — não há como aplicar um diff sem um estado de partida.
    #[error("order book has not been seeded with a snapshot yet, cannot apply deltas")]
    BookNotSeeded,
}
