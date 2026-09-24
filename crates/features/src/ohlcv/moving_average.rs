//! Médias móveis simples (SMA) e exponencial (EMA) do preço de fechamento.
//!
//! **Significado**: suavizam o preço para revelar tendência, filtrando
//! ruído de curto prazo. SMA pesa todas as barras da janela igualmente; EMA
//! dá mais peso às barras recentes, reagindo mais rápido a mudanças.
//!
//! **Fórmula**:
//! - `SMA_N = média dos últimos N closes`.
//! - `EMA_N`: `alpha = 2/(N+1)`; `EMA_t = alpha*close_t + (1-alpha)*EMA_{t-1}`.
//!
//! **Limitações**: ambas são indicadores *atrasados* (lagging) — reagem
//! depois que o preço já se moveu, nunca antecipam reversões. A EMA nunca
//! "esquece" totalmente o passado (o peso de uma barra antiga decai
//! geometricamente, mas nunca chega a zero), então após um choque de preço
//! ela carrega uma influência residual dele por muitas barras.

use crate::primitives::{Ema, RollingWindow};

#[derive(Debug, Clone)]
pub struct Sma {
    window: RollingWindow,
}

impl Sma {
    pub fn new(period: usize) -> Self {
        Self {
            window: RollingWindow::new(period),
        }
    }

    /// `None` até a janela encher (menos que `period` candles alimentados).
    pub fn update(&mut self, close: f64) -> Option<f64> {
        self.window.push(close);
        if self.window.is_full() {
            self.window.mean()
        } else {
            None
        }
    }
}

/// Reexportado como o tipo canônico de EMA para features OHLCV — mesma
/// implementação de `crate::primitives::Ema`, sem valor `None` inicial
/// (EMA sempre produz um valor a partir do primeiro candle, igual ao
/// próprio preço).
pub type EmaFeature = Ema;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sma_is_none_until_window_full() {
        let mut sma = Sma::new(3);
        assert_eq!(sma.update(1.0), None);
        assert_eq!(sma.update(2.0), None);
        assert_eq!(sma.update(3.0), Some(2.0));
    }

    #[test]
    fn sma_slides_after_full() {
        let mut sma = Sma::new(3);
        sma.update(1.0);
        sma.update(2.0);
        sma.update(3.0);
        // janela agora é [2,3,4]
        assert_eq!(sma.update(4.0), Some(3.0));
    }
}
