//! Autocorrelação (Pearson) dos retornos log com uma defasagem (lag) fixa.
//!
//! **Significado**: mede se o retorno de uma barra tende a se repetir
//! (autocorrelação positiva — momentum/tendência de curtíssimo prazo) ou a
//! se inverter (autocorrelação negativa — reversão à média) em relação a
//! `lag` barras atrás. Perto de zero sugere retornos aproximadamente
//! independentes bar-a-bar.
//!
//! **Fórmula**: correlação de Pearson entre as séries `retorno[0..N-lag]`
//! e `retorno[lag..N]`, onde `retorno` é a sequência de retornos log dentro
//! da janela de `período` retornos mais recentes.
//!
//! **Limitações**: exige `período + lag` retornos (`período + lag + 1`
//! candles) antes do primeiro valor. Indefinida quando um dos dois
//! subvetores tem variância zero (retornos constantes) — `update` devolve
//! `None` nesse caso em vez de dividir por zero. Um único lag fixo por
//! instância; para múltiplos lags, use uma instância por lag.

use crate::primitives::RollingWindow;

#[derive(Debug, Clone)]
pub struct Autocorrelation {
    lag: usize,
    returns: RollingWindow,
    prev_close: Option<f64>,
}

impl Autocorrelation {
    pub fn new(period: usize, lag: usize) -> Self {
        assert!(lag > 0, "autocorrelation lag must be positive");
        assert!(period > lag, "autocorrelation period must exceed lag");
        Self {
            lag,
            returns: RollingWindow::new(period),
            prev_close: None,
        }
    }

    pub fn update(&mut self, close: f64) -> Option<f64> {
        let Some(prev) = self.prev_close else {
            self.prev_close = Some(close);
            return None;
        };
        self.prev_close = Some(close);
        if prev <= 0.0 || close <= 0.0 {
            return None;
        }
        self.returns.push((close / prev).ln());

        if !self.returns.is_full() {
            return None;
        }
        let values = self.returns.to_vec();
        let n = values.len();
        let a = &values[..n - self.lag];
        let b = &values[self.lag..];
        pearson_correlation(a, b)
    }
}

fn pearson_correlation(a: &[f64], b: &[f64]) -> Option<f64> {
    debug_assert_eq!(a.len(), b.len());
    let n = a.len() as f64;
    let mean_a = a.iter().sum::<f64>() / n;
    let mean_b = b.iter().sum::<f64>() / n;

    let mut cov = 0.0;
    let mut var_a = 0.0;
    let mut var_b = 0.0;
    for i in 0..a.len() {
        let da = a[i] - mean_a;
        let db = b[i] - mean_b;
        cov += da * db;
        var_a += da * da;
        var_b += db * db;
    }

    if var_a == 0.0 || var_b == 0.0 {
        return None;
    }
    Some(cov / (var_a.sqrt() * var_b.sqrt()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfectly_repeating_returns_have_autocorrelation_one() {
        // Retornos log alternando entre dois valores fixos em padrão
        // period-2 -> lag-2 autocorrelação perfeita (cada retorno igual ao
        // de duas barras atrás).
        let mut ac = Autocorrelation::new(4, 2);
        let prices = [100.0, 110.0, 90.0, 99.0, 81.0, 89.1, 72.9];
        let mut last = None;
        for p in prices {
            last = ac.update(p);
        }
        let value = last.expect("expected a value after enough history");
        assert!((value - 1.0).abs() < 1e-6);
    }

    #[test]
    fn none_before_enough_history() {
        let mut ac = Autocorrelation::new(3, 1);
        assert_eq!(ac.update(100.0), None);
        assert_eq!(ac.update(101.0), None);
        assert_eq!(ac.update(102.0), None);
        assert!(ac.update(103.0).is_some());
    }
}
