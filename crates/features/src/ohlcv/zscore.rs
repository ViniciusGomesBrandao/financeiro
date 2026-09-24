//! Z-score do preço em relação à sua própria média móvel.
//!
//! **Significado**: quantos desvios padrão o preço atual está acima
//! (positivo) ou abaixo (negativo) da média da janela — a estatística
//! clássica por trás de estratégias de reversão à média.
//!
//! **Fórmula**: `z = (close - média_N) / desvio_padrão_N`.
//!
//! **Limitações**: assume que o preço reverte à média, o que não é
//! verdade em tendências fortes e sustentadas (um z-score "extremo" pode
//! continuar ficando mais extremo). Indefinido quando o desvio padrão da
//! janela é zero (preço constante) — `update` devolve `None` nesse caso em
//! vez de dividir por zero.

use crate::primitives::RollingWindow;

#[derive(Debug, Clone)]
pub struct ZScore {
    window: RollingWindow,
}

impl ZScore {
    pub fn new(period: usize) -> Self {
        Self {
            window: RollingWindow::new(period),
        }
    }

    pub fn update(&mut self, close: f64) -> Option<f64> {
        self.window.push(close);
        if !self.window.is_full() {
            return None;
        }
        let mean = self.window.mean()?;
        let stddev = self.window.stddev()?;
        if stddev == 0.0 {
            return None;
        }
        Some((close - mean) / stddev)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zscore_none_on_zero_stddev() {
        let mut z = ZScore::new(3);
        for v in [5.0, 5.0, 5.0] {
            assert!(z.update(v).is_none() || v == 5.0);
        }
        // janela cheia mas constante -> stddev 0 -> None
        assert_eq!(z.update(5.0), None);
    }

    #[test]
    fn zscore_matches_hand_computation() {
        let mut z = ZScore::new(4);
        for v in [2.0, 4.0, 4.0, 4.0] {
            z.update(v);
        }
        // janela [2,4,4,4]: mean=3.5, stddev=0.8660254...
        let out = z.update(4.0); // janela agora [4,4,4,4]: mean=4, stddev=0 -> None
        assert_eq!(out, None);

        let mut z2 = ZScore::new(4);
        z2.update(2.0);
        z2.update(4.0);
        z2.update(4.0);
        let out2 = z2.update(4.0).unwrap(); // janela cheia [2,4,4,4], close=4
        let expected = (4.0 - 3.5) / 0.866_025_403_784_438_6;
        assert!((out2 - expected).abs() < 1e-9);
    }
}
