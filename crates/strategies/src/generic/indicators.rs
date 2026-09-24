use std::collections::VecDeque;

/// Média móvel exponencial, atualizada incrementalmente (O(1) por tick, sem
/// retenção de histórico). Os valores são `f64` simples: são leituras
/// estatísticas de indicadores, não dinheiro, então a exatidão de `Decimal` não
/// é necessária e `f64` mantém a aritmética simples.
#[derive(Debug, Clone)]
pub struct Ema {
    alpha: f64,
    value: Option<f64>,
}

impl Ema {
    pub fn new(period: usize) -> Self {
        assert!(period > 0, "EMA period must be positive");
        Self {
            alpha: 2.0 / (period as f64 + 1.0),
            value: None,
        }
    }

    /// Alimenta um novo preço e retorna o valor atualizado da EMA.
    pub fn update(&mut self, price: f64) -> f64 {
        let next = match self.value {
            Some(prev) => self.alpha * price + (1.0 - self.alpha) * prev,
            None => price,
        };
        self.value = Some(next);
        next
    }

    pub fn value(&self) -> Option<f64> {
        self.value
    }
}

/// Uma janela móvel de capacidade fixa com valores recentes, usada pelas
/// estratégias de momentum e de reversão à média para calcular estatísticas do
/// período de lookback.
#[derive(Debug, Clone)]
pub struct RollingWindow {
    capacity: usize,
    values: VecDeque<f64>,
}

impl RollingWindow {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "window capacity must be positive");
        Self {
            capacity,
            values: VecDeque::with_capacity(capacity),
        }
    }

    pub fn push(&mut self, value: f64) {
        if self.values.len() == self.capacity {
            self.values.pop_front();
        }
        self.values.push_back(value);
    }

    pub fn is_full(&self) -> bool {
        self.values.len() == self.capacity
    }

    pub fn first(&self) -> Option<f64> {
        self.values.front().copied()
    }

    pub fn last(&self) -> Option<f64> {
        self.values.back().copied()
    }

    pub fn mean(&self) -> Option<f64> {
        if self.values.is_empty() {
            return None;
        }
        Some(self.values.iter().sum::<f64>() / self.values.len() as f64)
    }

    /// Desvio padrão populacional do conteúdo da janela.
    pub fn stddev(&self) -> Option<f64> {
        let mean = self.mean()?;
        if self.values.len() < 2 {
            return Some(0.0);
        }
        let variance =
            self.values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / self.values.len() as f64;
        Some(variance.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ema_first_update_equals_input() {
        let mut ema = Ema::new(10);
        assert_eq!(ema.update(100.0), 100.0);
    }

    #[test]
    fn ema_converges_toward_constant_input() {
        let mut ema = Ema::new(5);
        let mut last = ema.update(10.0);
        for _ in 0..50 {
            last = ema.update(20.0);
        }
        assert!((last - 20.0).abs() < 0.01);
    }

    #[test]
    fn rolling_window_evicts_oldest() {
        let mut window = RollingWindow::new(3);
        window.push(1.0);
        window.push(2.0);
        window.push(3.0);
        assert!(window.is_full());
        assert_eq!(window.first(), Some(1.0));
        window.push(4.0);
        assert_eq!(window.first(), Some(2.0));
        assert_eq!(window.last(), Some(4.0));
    }

    #[test]
    fn rolling_window_mean_and_stddev() {
        let mut window = RollingWindow::new(4);
        for v in [2.0, 4.0, 4.0, 4.0] {
            window.push(v);
        }
        assert_eq!(window.mean(), Some(3.5));
        // desvio padrão populacional de [2,4,4,4]
        let stddev = window.stddev().unwrap();
        assert!((stddev - 0.866_025_403_784_438_6).abs() < 1e-9);
    }
}
