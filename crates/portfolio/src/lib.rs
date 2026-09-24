//! Detém o estado do portfólio (caixa, posições, P&L) da simulação de
//! paper trading. Veja `PortfolioManager` para a fonte única de verdade que
//! este crate existe para fornecer — tanto o motor de risco quanto o paper
//! broker leem dele ou escrevem através dele, em vez de manterem cópias
//! próprias.

pub mod error;
pub mod manager;

pub use error::PortfolioError;
pub use manager::PortfolioManager;
