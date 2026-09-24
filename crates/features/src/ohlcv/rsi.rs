//! Relative Strength Index (RSI) de Wilder.
//!
//! **Significado**: oscilador de momentum em `[0, 100]` — valores altos
//! (convencionalmente > 70) são lidos como "sobrecomprado", baixos
//! (< 30) como "sobrevendido". Mede a *proporção* entre ganhos e perdas
//! médios recentes, não sua magnitude absoluta.
//!
//! **Fórmula**: por candle, `ganho = max(close - close_anterior, 0)`,
//! `perda = max(close_anterior - close, 0)`; `média_ganho`/`média_perda`
//! são suavizações de Wilder (ver `primitives::WilderAverage`) desses dois
//! valores; `RS = média_ganho / média_perda`; `RSI = 100 - 100/(1+RS)`.
//!
//! **Limitações**: indefinido no primeiro candle (sem `close_anterior`) e
//! só produz um valor "oficial" após `período` candles, como toda média de
//! Wilder. Casos de borda: se `média_perda == 0` e `média_ganho == 0` (sem
//! nenhum movimento na janela), o RSI é reportado como `50` (neutro, uma
//! convenção, não uma derivação matemática de `0/0`); se `média_perda == 0`
//! mas `média_ganho > 0`, o RSI é `100`.

use crate::primitives::WilderAverage;

#[derive(Debug, Clone)]
pub struct Rsi {
    avg_gain: WilderAverage,
    avg_loss: WilderAverage,
    prev_close: Option<f64>,
}

impl Rsi {
    pub fn new(period: usize) -> Self {
        Self {
            avg_gain: WilderAverage::new(period),
            avg_loss: WilderAverage::new(period),
            prev_close: None,
        }
    }

    pub fn update(&mut self, close: f64) -> Option<f64> {
        let Some(prev) = self.prev_close else {
            self.prev_close = Some(close);
            return None;
        };
        self.prev_close = Some(close);

        let delta = close - prev;
        let gain = delta.max(0.0);
        let loss = (-delta).max(0.0);

        let avg_gain = self.avg_gain.update(gain);
        let avg_loss = self.avg_loss.update(loss);

        match (avg_gain, avg_loss) {
            (Some(g), Some(l)) if g == 0.0 && l == 0.0 => Some(50.0),
            (Some(_), Some(0.0)) => Some(100.0),
            (Some(g), Some(l)) => Some(100.0 - 100.0 / (1.0 + g / l)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_on_first_candle_and_before_period() {
        let mut rsi = Rsi::new(2);
        assert_eq!(rsi.update(100.0), None); // sem close anterior
        assert_eq!(rsi.update(101.0), None); // 1 delta, período=2
        assert!(rsi.update(102.0).is_some());
    }

    #[test]
    fn all_gains_yields_rsi_100() {
        let mut rsi = Rsi::new(2);
        rsi.update(100.0);
        rsi.update(101.0);
        let out = rsi.update(102.0).unwrap();
        assert_eq!(out, 100.0);
    }

    #[test]
    fn all_losses_yields_rsi_0() {
        let mut rsi = Rsi::new(2);
        rsi.update(100.0);
        rsi.update(99.0);
        let out = rsi.update(98.0).unwrap();
        assert_eq!(out, 0.0);
    }

    #[test]
    fn no_movement_yields_neutral_50() {
        let mut rsi = Rsi::new(2);
        rsi.update(100.0);
        rsi.update(100.0);
        let out = rsi.update(100.0).unwrap();
        assert_eq!(out, 50.0);
    }
}
