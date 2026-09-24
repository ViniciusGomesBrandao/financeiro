//! Benchmark simples: retorno de comprar e segurar (buy & hold) o próprio
//! ativo pelo mesmo período do backtest — o piso de comparação mais básico
//! para qualquer estratégia ativa.

use domain::Candle;
use rust_decimal::Decimal;

/// `(close do último candle - close do primeiro) / close do primeiro`.
/// `None` se `candles` estiver vazio ou o primeiro `close` for zero.
pub fn compute_buy_and_hold_return(candles: &[Candle]) -> Option<Decimal> {
    let first = candles.first()?;
    let last = candles.last()?;
    if first.close == Decimal::ZERO {
        return None;
    }
    Some((last.close - first.close) / first.close)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use domain::{InstrumentId, Timeframe};
    use rust_decimal_macros::dec;

    fn candle(close: Decimal) -> Candle {
        let now = Utc::now();
        Candle {
            instrument_id: InstrumentId::new(),
            timeframe: Timeframe::D1,
            open_time: now,
            close_time: now,
            open: close,
            high: close,
            low: close,
            close,
            volume: dec!(1),
            is_closed: true,
        }
    }

    #[test]
    fn computes_percentage_change_between_first_and_last_close() {
        let candles = vec![candle(dec!(100)), candle(dec!(150)), candle(dec!(120))];
        assert_eq!(compute_buy_and_hold_return(&candles), Some(dec!(0.2)));
    }

    #[test]
    fn empty_candles_yield_none() {
        assert_eq!(compute_buy_and_hold_return(&[]), None);
    }
}
