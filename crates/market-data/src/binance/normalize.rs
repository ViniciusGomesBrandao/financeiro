//! Converte os DTOs da Binance em tipos de `domain`. Esta é a costura
//! descrita no diagrama de fluxo de dados do README da raiz:
//!
//! ```text
//! Binance JSON -> Binance Adapter (este módulo) -> domain::Candle / domain::MarketTrade
//! ```
//!
//! Nada fora de `binance/` deve construir um `Candle` ou um `MarketTrade`
//! diretamente a partir dos campos crus da Binance.

use chrono::{DateTime, TimeZone, Utc};
use domain::{Candle, InstrumentId, MarketTrade, Side, Timeframe};
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::binance::dto::{KlineData, RawKline, TradeEventPayload};
use crate::error::MarketDataError;

fn parse_decimal(raw: &str) -> Result<Decimal, MarketDataError> {
    Decimal::from_str(raw)
        .map_err(|e| MarketDataError::Parse(format!("invalid decimal {raw:?}: {e}")))
}

fn millis_to_utc(ms: i64) -> Result<DateTime<Utc>, MarketDataError> {
    Utc.timestamp_millis_opt(ms)
        .single()
        .ok_or_else(|| MarketDataError::Parse(format!("invalid timestamp {ms}")))
}

pub fn raw_kline_to_candle(
    raw: &RawKline,
    instrument_id: InstrumentId,
    timeframe: Timeframe,
) -> Result<Candle, MarketDataError> {
    let close_time = millis_to_utc(raw.6)?;
    Ok(Candle {
        instrument_id,
        timeframe,
        open_time: millis_to_utc(raw.0)?,
        close_time,
        open: parse_decimal(&raw.1)?,
        high: parse_decimal(&raw.2)?,
        low: parse_decimal(&raw.3)?,
        close: parse_decimal(&raw.4)?,
        volume: parse_decimal(&raw.5)?,
        // `GET /api/v3/klines` consultado sem um `endTime` explícito (como
        // `fetch_klines` faz) pode retornar a barra em formação como sua
        // última linha — a Binance não a omite. Uma barra só está de fato
        // fechada quando seu `close_time` já passou; comparar com o horário
        // do relógio (em vez de sempre confiar em `true`) é o que evita que
        // um candle ainda aberto seja entregue a estratégias/backtests como
        // se fosse definitivo.
        is_closed: close_time <= Utc::now(),
    })
}

pub fn ws_kline_to_candle(
    payload: &KlineData,
    instrument_id: InstrumentId,
    timeframe: Timeframe,
) -> Result<Candle, MarketDataError> {
    Ok(Candle {
        instrument_id,
        timeframe,
        open_time: millis_to_utc(payload.open_time)?,
        close_time: millis_to_utc(payload.close_time)?,
        open: parse_decimal(&payload.open)?,
        high: parse_decimal(&payload.high)?,
        low: parse_decimal(&payload.low)?,
        close: parse_decimal(&payload.close)?,
        volume: parse_decimal(&payload.volume)?,
        is_closed: payload.is_closed,
    })
}

pub fn ws_trade_to_market_trade(
    payload: &TradeEventPayload,
    instrument_id: InstrumentId,
) -> Result<MarketTrade, MarketDataError> {
    Ok(MarketTrade {
        instrument_id,
        exchange_trade_id: payload.trade_id.to_string(),
        price: parse_decimal(&payload.price)?,
        quantity: parse_decimal(&payload.quantity)?,
        // A flag `m` da Binance é true quando o comprador é o maker, o que
        // significa que o trade foi agredido pelo vendedor.
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
    use chrono::Duration;

    #[test]
    fn parses_raw_kline_into_candle() {
        let raw = RawKline(
            1_499_040_000_000,
            "0.01634790".to_string(),
            "0.80000000".to_string(),
            "0.01575800".to_string(),
            "0.01577100".to_string(),
            "148976.11427815".to_string(),
            1_499_644_799_999,
            "2434.19055334".to_string(),
            308,
            "1756.87402397".to_string(),
            "28.05995764".to_string(),
            "0".to_string(),
        );
        let instrument_id = InstrumentId::new();
        let candle = raw_kline_to_candle(&raw, instrument_id, Timeframe::M1).unwrap();
        assert_eq!(candle.open, parse_decimal("0.01634790").unwrap());
        assert_eq!(candle.close, parse_decimal("0.01577100").unwrap());
        assert!(candle.is_closed);
    }

    #[test]
    fn raw_kline_with_future_close_time_is_not_closed() {
        // O endpoint `/klines` da Binance, consultado sem um `endTime`
        // explícito, pode retornar a barra em formação como sua última
        // linha. Uma linha cujo close_time ainda não ocorreu precisa ser
        // reportada como não fechada, e não tida cegamente como final.
        let future_close_ms = (Utc::now() + Duration::minutes(5)).timestamp_millis();
        let open_ms = future_close_ms - 60_000;
        let raw = RawKline(
            open_ms,
            "100".to_string(),
            "101".to_string(),
            "99".to_string(),
            "100.5".to_string(),
            "10".to_string(),
            future_close_ms,
            "1000".to_string(),
            5,
            "5".to_string(),
            "500".to_string(),
            "0".to_string(),
        );
        let candle = raw_kline_to_candle(&raw, InstrumentId::new(), Timeframe::M1).unwrap();
        assert!(!candle.is_closed);
    }

    #[test]
    fn maps_buyer_is_maker_to_taker_sell() {
        let payload = TradeEventPayload {
            symbol: "BTCUSDT".to_string(),
            trade_id: 12345,
            price: "50000.00".to_string(),
            quantity: "0.1".to_string(),
            trade_time: 1_699_000_000_000,
            buyer_is_maker: true,
        };
        let trade = ws_trade_to_market_trade(&payload, InstrumentId::new()).unwrap();
        assert_eq!(trade.taker_side, Side::Sell);
    }

    #[test]
    fn maps_buyer_is_not_maker_to_taker_buy() {
        let payload = TradeEventPayload {
            symbol: "BTCUSDT".to_string(),
            trade_id: 1,
            price: "1".to_string(),
            quantity: "1".to_string(),
            trade_time: 0,
            buyer_is_maker: false,
        };
        let trade = ws_trade_to_market_trade(&payload, InstrumentId::new()).unwrap();
        assert_eq!(trade.taker_side, Side::Buy);
    }
}
