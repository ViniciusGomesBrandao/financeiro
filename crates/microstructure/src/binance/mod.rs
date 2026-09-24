//! Adaptador para os streams públicos de microestrutura da Binance Spot
//! (depth diff, bookTicker, trade). Todo o encanamento de formato de
//! transmissão (`dto`, `normalize`, `symbol`) deve permanecer privado a
//! este módulo — só `BinanceDepthClient` e `BinanceMicrostructureWsClient`
//! são expostos.

mod dto;
mod normalize;
mod rest;
mod symbol;
mod ws;

pub use rest::BinanceDepthClient;
pub use symbol::wire_symbol;
pub use ws::{BinanceMicrostructureWsClient, MicrostructureWsEvent};
