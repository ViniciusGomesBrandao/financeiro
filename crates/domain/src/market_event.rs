use chrono::{DateTime, Utc};

use crate::market_data::{Candle, MarketTrade, OrderBookSnapshot};

/// Uma unidade de dado de mercado que entra no motor de estratégias, vinda
/// *tanto* do pipeline ao vivo *quanto* do runner de backtest. Estratégias
/// consomem `MarketEvent`s e nunca sabem se vieram de um WebSocket ou de um
/// replay histórico — este é o ponto de junção que permite ao mesmo código de
/// estratégia rodar ao vivo e em backtest.
#[derive(Debug, Clone, PartialEq)]
pub enum MarketEvent {
    Candle(Candle),
    Trade(MarketTrade),
    OrderBook(OrderBookSnapshot),
}

impl MarketEvent {
    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            MarketEvent::Candle(c) => c.close_time,
            MarketEvent::Trade(t) => t.timestamp,
            MarketEvent::OrderBook(b) => b.timestamp,
        }
    }
}
