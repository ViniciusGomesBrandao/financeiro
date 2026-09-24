//! Execução de ordens. Define o trait `Broker` e sua única implementação
//! nesta fase, `PaperBroker` — um broker totalmente simulado, sem dinheiro
//! real e sem conectividade com exchange real. Veja a documentação do
//! módulo `PaperBroker` para o modelo de fees/slippage que ele aplica.

pub mod broker;
pub mod config;
pub mod error;
pub mod paper_broker;
pub mod report;

pub use broker::Broker;
pub use config::PaperBrokerConfig;
pub use error::BrokerError;
pub use paper_broker::PaperBroker;
pub use report::{ExecutionReport, PositionEvent};
