//! Persistência em PostgreSQL via SQLx. Depende deliberadamente apenas de
//! `domain` — nenhum crate aqui precisa conhecer strategies, risk ou
//! execution para armazenar suas saídas.
//!
//! Toda query deste crate usa a API verificada em runtime do SQLx
//! (`sqlx::query`/`sqlx::query_as`), nunca as macros `query!` verificadas em
//! tempo de compilação. Essas macros exigem um banco ativo (ou um cache de
//! queries offline) disponível no momento do `cargo build`; exigir isso de
//! todo contribuidor e de todo ambiente de CI foi considerado um custo maior
//! que o benefício da checagem extra de tipos, para o estágio atual do
//! projeto.
//!
//! Apenas dados operacionais recentes são persistidos aqui (instruments,
//! signals, orders, fills, positions, trades, portfolio snapshots,
//! desempenho de estratégias) — histórico bruto de OHLCV/ticks *não* é
//! guardado no Postgres. Ver o README raiz, "Persistência", para o plano de
//! mover dados históricos em volume para Parquet quando o backtesting
//! precisar.

pub mod active_strategy_state;
pub mod codec;
pub mod db;
pub mod error;
pub mod fills;
pub mod instruments;
pub mod judge_telemetry;
pub mod latest_prices;
pub mod operational_robots;
pub mod orders;
pub mod portfolio_snapshots;
pub mod positions;
pub mod risk_decisions;
pub mod robot_portfolio_snapshots;
pub mod signals;
pub mod strategy_configs;
pub mod strategy_performance;
pub mod trades;

pub use db::{connect, run_migrations};
pub use error::PersistenceError;
