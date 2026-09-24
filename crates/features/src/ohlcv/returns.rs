//! Retornos simples multi-horizonte.
//!
//! **Significado**: variação percentual do preço de fechamento entre o
//! candle atual e o candle `período` barras atrás — a leitura mais direta
//! de "quanto o preço mudou" em cada horizonte configurado.
//!
//! **Fórmula**: `retorno_p = close_t / close_{t-p} - 1`.
//!
//! **Limitações**: "multi-timeframe" aqui significa múltiplos *lookbacks*
//! (em número de candles) sobre a mesma série de barras — por exemplo,
//! `[1, 5, 15, 60]` candles de 1 minuto — não candles reamostrados para
//! timeframes diferentes (1m/5m/1h agregados de verdade). Reamostragem de
//! timeframe é um recurso maior (precisa agregar OHLCV corretamente, não só
//! reindexar) e fica fora do escopo deste motor por ora. Cada horizonte só
//! aparece no snapshot quando há candles suficientes no histórico.

use crate::primitives::RollingWindow;

/// Mantém preços de fechamento suficientes para calcular retorno em todos
/// os horizontes configurados de uma vez (a janela é dimensionada para o
/// maior período + 1 barra).
#[derive(Debug, Clone)]
pub struct MultiHorizonReturns {
    periods: Vec<usize>,
    window: RollingWindow,
}

impl MultiHorizonReturns {
    pub fn new(periods: Vec<usize>) -> Self {
        let max_period = periods.iter().copied().max().unwrap_or(1);
        Self {
            periods,
            window: RollingWindow::new(max_period + 1),
        }
    }

    /// Alimenta um novo preço de fechamento e devolve `período -> retorno`
    /// para cada horizonte configurado que já tem histórico suficiente.
    pub fn update(&mut self, close: f64) -> std::collections::BTreeMap<usize, f64> {
        self.window.push(close);
        let values = self.window.to_vec();
        let n = values.len();

        let mut out = std::collections::BTreeMap::new();
        for &period in &self.periods {
            if period == 0 || n <= period {
                continue;
            }
            let past = values[n - 1 - period];
            let current = values[n - 1];
            if past != 0.0 {
                out.insert(period, current / past - 1.0);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_return_before_enough_history() {
        let mut r = MultiHorizonReturns::new(vec![1, 3]);
        assert!(r.update(100.0).is_empty());
        // Só 1 barra de histórico além da atual: período 1 já disponível,
        // período 3 ainda não.
        let out = r.update(110.0);
        assert_eq!(out.len(), 1);
        assert!((out[&1] - 0.10).abs() < 1e-12);
    }

    #[test]
    fn computes_multiple_horizons_once_available() {
        let mut r = MultiHorizonReturns::new(vec![1, 2]);
        r.update(100.0); // idx0
        r.update(110.0); // idx1, retorno_1 = 0.10
        let out = r.update(121.0); // idx2, retorno_1 = 0.10, retorno_2 = 0.21
        assert!((out[&1] - 0.10).abs() < 1e-12);
        assert!((out[&2] - 0.21).abs() < 1e-9);
    }
}
