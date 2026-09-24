//! Backtesting: reproduz dados históricos de `Candle` pelo mesmo pipeline
//! `MarketEvent -> Strategy -> Signal -> Risk -> Broker` que o loop live da
//! aplicação usa. Veja `BacktestRunner` para o ponto de entrada.
//!
//! Este módulo compartilha intencionalmente `MarketEvent` (de `domain`) com
//! o caminho live, em vez de definir seu próprio tipo de evento — é esse
//! tipo compartilhado que torna "o mesmo código de estratégia, em live ou
//! backtest" um fato, e não apenas uma intenção.

pub mod error;
pub mod report;
pub mod runner;

pub use error::BacktestError;
pub use report::BacktestReport;
pub use runner::BacktestRunner;
