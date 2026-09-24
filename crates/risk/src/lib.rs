//! Gestão de risco: transforma um `Signal` em um `OrderRequest`, ou o
//! rejeita.
//!
//! `Strategy` nunca conversa diretamente com `RiskEngine` e nunca vê
//! `PortfolioState` — é o laço condutor (em `app`, ou no runner de
//! backtest) que chama `RiskEngine::evaluate` para cada sinal emitido por
//! uma estratégia. Isso mantém explícita a fronteira dentro do pipeline:
//!
//! ```text
//! Signal -> RiskEngine::evaluate -> RiskDecision -> (Approved -> Broker)
//! ```

pub mod config;
pub mod decision;
pub mod engine;
pub mod portfolio_state;

pub use config::RiskConfig;
pub use decision::{ExitReason, RejectionReason, RiskDecision};
pub use engine::RiskEngine;
pub use portfolio_state::PortfolioState;
