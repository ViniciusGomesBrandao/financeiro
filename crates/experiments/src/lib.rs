//! Experiment runner: roda cada estratégia (3 baselines + 3
//! quantitativas) isoladamente contra dados históricos reais
//! (`historical-data`), através do `backtest::BacktestRunner` já
//! corrigido, produzindo um relatório de performance auditável por
//! combinação de instrumento/timeframe/estratégia.
//!
//! Este crate é **100% offline**: nunca depende de Postgres nem de nenhum
//! broker live — lê Parquet local, escreve JSON/Markdown/CSV local.
//!
//! Fluxo: `strategy_set` (as 6 fábricas de estratégia, hoje um wrapper
//! fino sobre `strategies::catalog` — ver o doc daquele módulo) + `matrix`
//! (config fixa de risco/paper trading) alimentam `runner`
//! (`run_single_experiment`, uma execução isolada) -> `report`
//! (`ExperimentReport`, serializável) -> `summary` (tabela comparativa
//! entre todas as execuções de uma invocação).

pub mod diagnostic;
pub mod error;
pub mod matrix;
pub mod report;
pub mod runner;
pub mod strategy_set;
pub mod summary;
pub mod window;

pub use error::ExperimentError;
pub use matrix::{default_config, DefaultConfig};
pub use report::ExperimentReport;
pub use runner::{load_candles, run_single_experiment};
pub use strategy_set::{all_factories, StrategyFactory};
pub use window::{slice_recent_window, WINDOWS};
