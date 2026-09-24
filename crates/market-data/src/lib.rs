//! Ingestão de dados de mercado, desacoplada de qualquer exchange
//! específica através de `MarketDataProvider`. A única implementação hoje é
//! `binance::BinanceMarketData`, que consome os endpoints públicos REST e
//! WebSocket do Binance Spot.
//!
//! Fluxo de dados (veja o `README.md` na raiz para o panorama completo):
//!
//! ```text
//! Binance JSON -> binance::normalize -> domain::Candle / domain::MarketTrade -> MarketEvent
//! ```
//!
//! Adicionar uma nova exchange significa adicionar um novo módulo irmão que
//! implemente `MarketDataProvider` — a superfície pública deste crate
//! (`provider`) nunca precisa mudar por causa disso.

pub mod binance;
pub mod error;
pub mod provider;

pub use binance::BinanceMarketData;
pub use error::MarketDataError;
pub use provider::{MarketDataProvider, ProviderCapabilities};
