use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExperimentError {
    #[error("historical data error: {0}")]
    HistoricalData(#[from] historical_data::HistoricalDataError),
    #[error("no local candle data for {symbol}/{timeframe} — run fetch-data first")]
    NoLocalData { symbol: String, timeframe: String },
    #[error("strategy registration failed: {0}")]
    Registration(#[from] strategies::CompatibilityError),
    #[error("backtest run failed: {0}")]
    Backtest(#[from] backtest::BacktestError),
    #[error("market data error: {0}")]
    MarketData(#[from] market_data::MarketDataError),
    #[error("failed to write report to {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to serialize report: {0}")]
    Serialize(#[from] serde_json::Error),
    /// Em modo strict (o modo dos experimentos oficiais — ver
    /// `runner::run_single_experiment`), um recorte de candles com
    /// gap/duplicata/fora-de-ordem nunca roda: o experimento falha antes
    /// de sequer iniciar o backtest, em vez de produzir um relatório sobre
    /// dados de qualidade desconhecida. Ver `historical_data::validate`
    /// para a definição exata de cada categoria.
    #[error(
        "{symbol}/{timeframe}/{window}: data quality check failed in strict mode — \
         {gaps} gaps ({missing_candles} candles missing), {duplicates} duplicates, \
         {out_of_order} out-of-order; re-run fetch-data or use a cleaner window"
    )]
    DataQuality {
        symbol: String,
        timeframe: String,
        window: String,
        gaps: usize,
        missing_candles: i64,
        duplicates: usize,
        out_of_order: usize,
    },
}
