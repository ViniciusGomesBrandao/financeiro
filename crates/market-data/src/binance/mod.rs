//! Adaptador para os dados públicos de mercado do Binance Spot. Veja
//! `provider::BinanceMarketData` para a implementação de
//! `MarketDataProvider`; todo o resto aqui é encanamento de formato de
//! transmissão específico da Binance, que deve permanecer privado a este
//! módulo (`dto`, `normalize`, `rest`, `ws`, `symbol`).

mod dto;
mod normalize;
mod provider;
mod rest;
mod symbol;
mod ws;

pub use provider::BinanceMarketData;
pub use rest::BinanceRestClient;
pub use ws::BinanceWsClient;
