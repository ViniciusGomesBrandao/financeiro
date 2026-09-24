//! Average True Range (ATR) — volatilidade medida em unidades de preço,
//! usando high/low/close (não só close, ao contrário do desvio padrão de
//! preço em `ohlcv::volatility`).
//!
//! **Significado**: o "alcance" médio que o preço realmente percorreu por
//! candle, incluindo gaps entre o fechamento anterior e o candle atual —
//! usado classicamente para dimensionar stops/posições proporcionalmente à
//! volatilidade real do instrumento.
//!
//! **Fórmula**: `true_range = max(high-low, |high-close_anterior|,
//! |low-close_anterior|)`; `ATR = suavização de Wilder do true_range`
//! (ver `primitives::WilderAverage` — `alpha = 1/período`, distinta de uma
//! EMA comum).
//!
//! **Limitações**: no primeiro candle não há `close_anterior`, então o
//! true range desse candle degenera para `high-low` (sem componente de
//! gap) — um leve viés de subestimação só na primeira barra da série.
//! Como toda média de Wilder, o primeiro valor "oficial" só existe após
//! `período` candles.

use crate::primitives::WilderAverage;

#[derive(Debug, Clone)]
pub struct Atr {
    wilder: WilderAverage,
    prev_close: Option<f64>,
}

impl Atr {
    pub fn new(period: usize) -> Self {
        Self {
            wilder: WilderAverage::new(period),
            prev_close: None,
        }
    }

    pub fn update(&mut self, high: f64, low: f64, close: f64) -> Option<f64> {
        let true_range = match self.prev_close {
            Some(prev) => (high - low)
                .max((high - prev).abs())
                .max((low - prev).abs()),
            None => high - low,
        };
        self.prev_close = Some(close);
        self.wilder.update(true_range)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_candle_true_range_is_high_minus_low() {
        let mut atr = Atr::new(1);
        // período 1 -> Wilder semeia no primeiro valor.
        let out = atr.update(110.0, 100.0, 105.0);
        assert_eq!(out, Some(10.0));
    }

    #[test]
    fn true_range_includes_gap_from_previous_close() {
        let mut atr = Atr::new(1);
        atr.update(110.0, 100.0, 105.0); // prev_close = 105
                                         // gap para cima: high=130, low=125 -> |high-prev|=25 domina (130-125=5, |125-105|=20)
        let out = atr.update(130.0, 125.0, 128.0);
        assert_eq!(out, Some(25.0));
    }

    #[test]
    fn none_before_period_candles() {
        let mut atr = Atr::new(3);
        assert_eq!(atr.update(110.0, 100.0, 105.0), None);
        assert_eq!(atr.update(112.0, 104.0, 108.0), None);
        assert!(atr.update(115.0, 107.0, 110.0).is_some());
    }
}
