//! Camada de dados históricos: baixa candles OHLCV reais da Binance Spot,
//! guarda 1m localmente em Parquet, deriva 5m/15m/1h a partir do 1m, e
//! valida ordem temporal/duplicatas/gaps — sem depender de Postgres nem de
//! nenhum outro componente operacional deste workspace. `experiments`
//! consome os arquivos que este crate produz para rodar backtests offline.
//!
//! Fluxo: `download` (paginação HTTP) -> `update` (orquestra
//! read-existing/merge/validate/write, incremental) -> `store` (Parquet
//! read/write) -> `aggregate` (1m -> timeframes maiores).

pub mod aggregate;
pub mod download;
pub mod error;
pub mod store;
pub mod symbol;
pub mod update;
pub mod validate;

pub use error::HistoricalDataError;
pub use update::{
    earliest_anchor, update_pair, update_symbol, validation_report_path, UpdateSummary,
    ValidationRecord, DERIVED_TIMEFRAMES,
};
pub use validate::{validate, GapRange, ValidationReport};
