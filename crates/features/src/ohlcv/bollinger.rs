//! Bandas de Bollinger.
//!
//! **Significado**: um envelope de volatilidade ao redor da média móvel —
//! banda central = SMA, bandas externas = SMA ± k desvios padrão. Preço
//! perto/fora da banda externa é lido como "esticado" em relação à sua
//! própria volatilidade recente; bandas estreitas costumam preceder
//! expansões de volatilidade ("squeeze").
//!
//! **Fórmula**: `média = SMA_N`; `upper = média + k*stddev_N`;
//! `lower = média - k*stddev_N`; `percent_b = (close-lower)/(upper-lower)`;
//! `bandwidth = (upper-lower)/média`.
//!
//! **Limitações**: mesma suposição de "reversão à média" do z-score (ver
//! `ohlcv::zscore`) — as bandas não preveem *quando* o preço vai reverter,
//! só descrevem a posição atual relativa à volatilidade recente.
//! `percent_b`/`bandwidth` ficam indefinidos quando `upper == lower`
//! (janela com desvio padrão zero) ou quando `média == 0`.

use crate::primitives::RollingWindow;
use crate::snapshot::BollingerBands;

#[derive(Debug, Clone)]
pub struct Bollinger {
    window: RollingWindow,
    k: f64,
}

impl Bollinger {
    pub fn new(period: usize, k: f64) -> Self {
        Self {
            window: RollingWindow::new(period),
            k,
        }
    }

    pub fn update(&mut self, close: f64) -> Option<BollingerBands> {
        self.window.push(close);
        if !self.window.is_full() {
            return None;
        }
        let middle = self.window.mean()?;
        let stddev = self.window.stddev()?;
        let upper = middle + self.k * stddev;
        let lower = middle - self.k * stddev;

        let band_range = upper - lower;
        if band_range == 0.0 || middle == 0.0 {
            return None;
        }
        Some(BollingerBands {
            middle,
            upper,
            lower,
            percent_b: (close - lower) / band_range,
            bandwidth: band_range / middle,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bollinger_matches_hand_computation() {
        let mut b = Bollinger::new(4, 2.0);
        b.update(2.0);
        b.update(4.0);
        b.update(4.0);
        let bands = b.update(4.0).unwrap(); // janela [2,4,4,4]
        let expected_stddev = 0.866_025_403_784_438_6;
        assert!((bands.middle - 3.5).abs() < 1e-9);
        assert!((bands.upper - (3.5 + 2.0 * expected_stddev)).abs() < 1e-9);
        assert!((bands.lower - (3.5 - 2.0 * expected_stddev)).abs() < 1e-9);
        assert!(bands.percent_b > 0.5 && bands.percent_b < 1.0); // close acima da média
    }

    #[test]
    fn bollinger_none_when_flat_window() {
        let mut b = Bollinger::new(3, 2.0);
        for v in [10.0, 10.0, 10.0] {
            b.update(v);
        }
        assert_eq!(b.update(10.0), None);
    }
}
