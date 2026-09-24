//! Converte os DTOs da Binance em tipos de `domain`/`book`. Nada fora de
//! `binance/` deve construir um `OrderBookDelta`, `BookTicker` ou
//! `DepthSnapshot` diretamente a partir dos campos crus da Binance.

use chrono::{DateTime, TimeZone, Utc};
use domain::{BookLevel, BookTicker, InstrumentId, MarketTrade, OrderBookDelta, Side};
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::binance::dto::{
    BookTickerPayload, DepthSnapshotResponse, DepthUpdatePayload, TradePayload,
};
use crate::book::DepthSnapshot;
use crate::error::MicrostructureError;

fn parse_decimal(raw: &str) -> Result<Decimal, MicrostructureError> {
    Decimal::from_str(raw)
        .map_err(|e| MicrostructureError::Parse(format!("invalid decimal {raw:?}: {e}")))
}

fn millis_to_utc(ms: i64) -> Result<DateTime<Utc>, MicrostructureError> {
    Utc.timestamp_millis_opt(ms)
        .single()
        .ok_or_else(|| MicrostructureError::Parse(format!("invalid timestamp {ms}")))
}

fn parse_levels(raw: &[(String, String)]) -> Result<Vec<BookLevel>, MicrostructureError> {
    raw.iter()
        .map(|(price, quantity)| {
            Ok(BookLevel {
                price: parse_decimal(price)?,
                quantity: parse_decimal(quantity)?,
            })
        })
        .collect()
}

pub fn depth_snapshot_response_to_snapshot(
    raw: &DepthSnapshotResponse,
    instrument_id: InstrumentId,
    captured_at: DateTime<Utc>,
) -> Result<DepthSnapshot, MicrostructureError> {
    Ok(DepthSnapshot {
        instrument_id,
        last_update_id: raw.last_update_id,
        bids: parse_levels(&raw.bids)?,
        asks: parse_levels(&raw.asks)?,
        captured_at,
    })
}

pub fn ws_depth_update_to_delta(
    payload: &DepthUpdatePayload,
    instrument_id: InstrumentId,
    received_at: DateTime<Utc>,
) -> Result<OrderBookDelta, MicrostructureError> {
    Ok(OrderBookDelta {
        instrument_id,
        first_update_id: payload.first_update_id,
        final_update_id: payload.final_update_id,
        bids: parse_levels(&payload.bids)?,
        asks: parse_levels(&payload.asks)?,
        // A Binance inclui um horário de evento (`"E"`) no `depthUpdate`,
        // mas não em todas as versões/documentações de forma consistente —
        // usamos o horário de recebimento local, que é o que importa para
        // ordenar a sequência de aplicação (a ordem dos update ids é a
        // fonte de verdade, não o timestamp).
        timestamp: received_at,
    })
}

pub fn ws_book_ticker_to_domain(
    payload: &BookTickerPayload,
    instrument_id: InstrumentId,
    received_at: DateTime<Utc>,
) -> Result<BookTicker, MicrostructureError> {
    Ok(BookTicker {
        instrument_id,
        update_id: payload.update_id,
        bid_price: parse_decimal(&payload.bid_price)?,
        bid_qty: parse_decimal(&payload.bid_qty)?,
        ask_price: parse_decimal(&payload.ask_price)?,
        ask_qty: parse_decimal(&payload.ask_qty)?,
        // O stream `bookTicker` não carrega horário de evento — ver o doc
        // de `BookTickerPayload`. `received_at` é o horário local de
        // recebimento, não um horário de exchange.
        timestamp: received_at,
    })
}

pub fn ws_trade_to_market_trade(
    payload: &TradePayload,
    instrument_id: InstrumentId,
) -> Result<MarketTrade, MicrostructureError> {
    Ok(MarketTrade {
        instrument_id,
        exchange_trade_id: payload.trade_id.to_string(),
        price: parse_decimal(&payload.price)?,
        quantity: parse_decimal(&payload.quantity)?,
        // Mesma convenção de `market_data::binance::normalize`: `m=true`
        // significa que o comprador foi o maker, ou seja, o trade foi
        // agredido pelo vendedor.
        taker_side: if payload.buyer_is_maker {
            Side::Sell
        } else {
            Side::Buy
        },
        timestamp: millis_to_utc(payload.trade_time)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_depth_snapshot_response() {
        let raw = DepthSnapshotResponse {
            last_update_id: 12345,
            bids: vec![("100.50".to_string(), "1.5".to_string())],
            asks: vec![("101.00".to_string(), "2.0".to_string())],
        };
        let instrument_id = InstrumentId::new();
        let snapshot =
            depth_snapshot_response_to_snapshot(&raw, instrument_id, Utc::now()).unwrap();
        assert_eq!(snapshot.last_update_id, 12345);
        assert_eq!(snapshot.bids[0].price, parse_decimal("100.50").unwrap());
        assert_eq!(snapshot.asks[0].quantity, parse_decimal("2.0").unwrap());
    }

    #[test]
    fn maps_buyer_is_maker_to_taker_sell() {
        let payload = TradePayload {
            symbol: "BTCUSDT".to_string(),
            trade_id: 1,
            price: "50000".to_string(),
            quantity: "0.1".to_string(),
            trade_time: 1_700_000_000_000,
            buyer_is_maker: true,
        };
        let trade = ws_trade_to_market_trade(&payload, InstrumentId::new()).unwrap();
        assert_eq!(trade.taker_side, Side::Sell);
    }

    #[test]
    fn book_ticker_maps_all_fields() {
        let payload = BookTickerPayload {
            update_id: 42,
            symbol: "BTCUSDT".to_string(),
            bid_price: "100".to_string(),
            bid_qty: "1".to_string(),
            ask_price: "101".to_string(),
            ask_qty: "2".to_string(),
        };
        let ticker = ws_book_ticker_to_domain(&payload, InstrumentId::new(), Utc::now()).unwrap();
        assert_eq!(ticker.update_id, 42);
        assert_eq!(ticker.bid_price, parse_decimal("100").unwrap());
        assert_eq!(ticker.ask_qty, parse_decimal("2").unwrap());
    }
}
