//! Momentum / Rate of Change (ROC) — um único horizonte fixo, expresso em
//! percentual, distinto dos retornos multi-horizonte de `ohlcv::returns`
//! (que são fração, não percentual, e cobrem vários horizontes ao mesmo
//! tempo). Mantido como feature separada por ser o nome/formato
//! convencional em análise técnica (osciladores de momentum costumam
//! trabalhar em `%`, não em fração).
//!
//! **Significado**: velocidade da variação de preço no horizonte
//! configurado — positivo e crescente sugere momentum de alta se
//! fortalecendo; aproximando de zero sugere perda de força.
//!
//! **Fórmula**: `ROC = (close_t - close_{t-período}) / close_{t-período} * 100`.
//!
//! **Limitações**: mesma limitação de qualquer indicador de momentum —
//! não distingue "continuação" de "exaustão" (um ROC muito alto tanto pode
//! preceder mais alta quanto uma reversão). Indefinido se
//! `close_{t-período} == 0`.

use crate::primitives::RollingWindow;

#[derive(Debug, Clone)]
pub struct Roc {
    window: RollingWindow,
}

impl Roc {
    pub fn new(period: usize) -> Self {
        Self {
            window: RollingWindow::new(period + 1),
        }
    }

    pub fn update(&mut self, close: f64) -> Option<f64> {
        self.window.push(close);
        if !self.window.is_full() {
            return None;
        }
        let past = self.window.first()?;
        if past == 0.0 {
            return None;
        }
        Some((close - past) / past * 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roc_matches_hand_computation() {
        let mut roc = Roc::new(2);
        assert_eq!(roc.update(100.0), None);
        assert_eq!(roc.update(105.0), None);
        let out = roc.update(110.0).unwrap(); // (110-100)/100*100 = 10
        assert!((out - 10.0).abs() < 1e-12);
    }
}
