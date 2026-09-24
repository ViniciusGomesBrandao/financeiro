use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::instrument::InstrumentId;
use crate::side::Side;
use crate::timeframe::Timeframe;

/// Um candle OHLCV completo (ou em andamento), já normalizado a partir do
/// formato de transmissão que o adaptador da exchange recebeu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candle {
    pub instrument_id: InstrumentId,
    pub timeframe: Timeframe,
    pub open_time: DateTime<Utc>,
    pub close_time: DateTime<Utc>,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    /// `false` enquanto a janela do candle ainda está aberta (atualizações
    /// parciais via streaming); `true` quando a exchange fecha a barra.
    pub is_closed: bool,
}

/// Um único trade executado no mercado (tick).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketTrade {
    pub instrument_id: InstrumentId,
    /// Id do trade atribuído pela exchange, mantido como string opaca já que os
    /// formatos variam por venue (numérico na Binance, alfanumérico em outros).
    pub exchange_trade_id: String,
    pub price: Decimal,
    pub quantity: Decimal,
    /// Lado da ordem agressora (taker).
    pub taker_side: Side,
    pub timestamp: DateTime<Utc>,
}

/// Um único nível de preço em um livro de ofertas.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BookLevel {
    pub price: Decimal,
    pub quantity: Decimal,
}

/// Um snapshot do livro de ofertas com melhor bid/melhor ask (L1) ou com
/// profundidade (L2). Os níveis são ordenados do melhor para o pior (maior bid
/// primeiro, menor ask primeiro).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderBookSnapshot {
    pub instrument_id: InstrumentId,
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
    pub timestamp: DateTime<Utc>,
}

impl OrderBookSnapshot {
    pub fn best_bid(&self) -> Option<BookLevel> {
        self.bids.first().copied()
    }

    pub fn best_ask(&self) -> Option<BookLevel> {
        self.asks.first().copied()
    }

    pub fn mid_price(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid.price + ask.price) / Decimal::TWO),
            _ => None,
        }
    }
}

/// Um evento de diff do livro de ofertas (`depthUpdate` da Binance Spot).
/// Um nível em `bids`/`asks` com `quantity` zero significa "remover este
/// nível do livro", não "nível existente com quantidade zero" — a mesma
/// convenção que a própria Binance usa no payload.
///
/// **Sem campo `pu`/"previous update id"**: esse campo só existe no diff
/// depth de *Futures* da Binance. Para Spot (o único mercado que este
/// projeto usa), a checagem de continuidade entre dois eventos consecutivos
/// é `first_update_id do evento N == final_update_id do evento N-1 + 1` —
/// ver `microstructure::book::LocalOrderBook`, que é quem aplica essa
/// checagem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderBookDelta {
    pub instrument_id: InstrumentId,
    /// `"U"` no payload da Binance: primeiro update id coberto por este evento.
    pub first_update_id: i64,
    /// `"u"` no payload da Binance: último update id coberto por este evento.
    pub final_update_id: i64,
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
    pub timestamp: DateTime<Utc>,
}

/// Melhor bid/ask em tempo real (stream `bookTicker` da Binance) — mais
/// rápido que o diff de profundidade completo, mas sem níveis além do
/// topo do livro.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BookTicker {
    pub instrument_id: InstrumentId,
    pub update_id: i64,
    pub bid_price: Decimal,
    pub bid_qty: Decimal,
    pub ask_price: Decimal,
    pub ask_qty: Decimal,
    pub timestamp: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instrument::InstrumentId;
    use chrono::Utc;
    use rust_decimal_macros::dec;

    #[test]
    fn mid_price_averages_best_levels() {
        let book = OrderBookSnapshot {
            instrument_id: InstrumentId::new(),
            bids: vec![BookLevel {
                price: dec!(100),
                quantity: dec!(1),
            }],
            asks: vec![BookLevel {
                price: dec!(102),
                quantity: dec!(1),
            }],
            timestamp: Utc::now(),
        };
        assert_eq!(book.mid_price(), Some(dec!(101)));
    }

    #[test]
    fn mid_price_none_when_side_missing() {
        let book = OrderBookSnapshot {
            instrument_id: InstrumentId::new(),
            bids: vec![],
            asks: vec![],
            timestamp: Utc::now(),
        };
        assert_eq!(book.mid_price(), None);
    }
}
