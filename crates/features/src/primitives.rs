//! Blocos de construção estatísticos reutilizáveis por várias features —
//! nenhum deles é uma feature em si (não aparece em `FeatureSnapshot`
//! diretamente), só a mecânica incremental que as features montam por cima.

use std::collections::VecDeque;

/// Janela móvel de capacidade fixa. Descarta o valor mais antigo ao
/// ultrapassar a capacidade — O(1) por `push` (`VecDeque`), O(n) para
/// estatísticas que precisam varrer o conteúdo (`mean`, `stddev`,
/// `to_vec`), onde n é a capacidade da janela (tipicamente pequena: 10 a
/// 200), não o histórico inteiro.
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

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
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

    pub fn iter(&self) -> impl Iterator<Item = f64> + '_ {
        self.values.iter().copied()
    }

    pub fn to_vec(&self) -> Vec<f64> {
        self.values.iter().copied().collect()
    }

    pub fn sum(&self) -> f64 {
        self.values.iter().sum()
    }

    pub fn mean(&self) -> Option<f64> {
        if self.values.is_empty() {
            return None;
        }
        Some(self.sum() / self.values.len() as f64)
    }

    /// Desvio padrão **populacional** (divide por n, não por n-1) do
    /// conteúdo da janela — escolha deliberada para casar com
    /// `strategies::generic::indicators::RollingWindow::stddev`, já que
    /// aqui a janela inteira é sempre "a população" que a feature observa,
    /// não uma amostra de uma população maior.
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

/// Média móvel exponencial incremental — `alpha = 2/(period+1)`, O(1) por
/// atualização, sem reter histórico.
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

/// Suavização de Wilder (`alpha = 1/period`) — a média móvel usada
/// classicamente por RSI e ATR, distinta da EMA comum (`alpha =
/// 2/(period+1)`). Os primeiros `period` valores são acumulados como média
/// simples (a "semente"); a partir do `period`-ésimo valor, cada novo dado
/// suaviza o valor anterior. Isso significa que o primeiro valor "oficial"
/// de Wilder só existe depois de `period` observações — antes disso,
/// `value()` retorna `None`, nunca um número calculado sobre uma janela
/// incompleta.
#[derive(Debug, Clone)]
pub struct WilderAverage {
    period: usize,
    seed_sum: f64,
    seed_count: usize,
    value: Option<f64>,
}

impl WilderAverage {
    pub fn new(period: usize) -> Self {
        assert!(period > 0, "Wilder average period must be positive");
        Self {
            period,
            seed_sum: 0.0,
            seed_count: 0,
            value: None,
        }
    }

    pub fn update(&mut self, x: f64) -> Option<f64> {
        if let Some(prev) = self.value {
            let next = prev + (x - prev) / self.period as f64;
            self.value = Some(next);
            return Some(next);
        }
        self.seed_sum += x;
        self.seed_count += 1;
        if self.seed_count == self.period {
            let seeded = self.seed_sum / self.period as f64;
            self.value = Some(seeded);
            return Some(seeded);
        }
        None
    }

    pub fn value(&self) -> Option<f64> {
        self.value
    }
}

/// Variância EWMA no estilo RiskMetrics: `var_t = lambda*var_{t-1} +
/// (1-lambda)*x_t^2`. Alimentada com *retornos* (não preços), produz a
/// variância de um dia/barra para a EWMA volatility (ver
/// `ohlcv::volatility`). O primeiro retorno alimentado semeia a variância
/// diretamente como `x_0^2` — não há burn-in: a primeira leitura de
/// volatilidade é sempre menos confiável que as seguintes, uma limitação
/// inerente a qualquer EWMA sem um período de aquecimento separado.
#[derive(Debug, Clone)]
pub struct EwmaVariance {
    lambda: f64,
    variance: Option<f64>,
}

impl EwmaVariance {
    pub fn new(lambda: f64) -> Self {
        assert!(
            (0.0..1.0).contains(&lambda),
            "EWMA lambda must be in [0, 1)"
        );
        Self {
            lambda,
            variance: None,
        }
    }

    pub fn update(&mut self, x: f64) -> f64 {
        let next = match self.variance {
            Some(prev) => self.lambda * prev + (1.0 - self.lambda) * x * x,
            None => x * x,
        };
        self.variance = Some(next);
        next
    }

    pub fn variance(&self) -> Option<f64> {
        self.variance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_window_evicts_oldest() {
        let mut w = RollingWindow::new(3);
        w.push(1.0);
        w.push(2.0);
        w.push(3.0);
        assert!(w.is_full());
        w.push(4.0);
        assert_eq!(w.to_vec(), vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn rolling_window_mean_and_stddev() {
        let mut w = RollingWindow::new(4);
        for v in [2.0, 4.0, 4.0, 4.0] {
            w.push(v);
        }
        assert_eq!(w.mean(), Some(3.5));
        assert!((w.stddev().unwrap() - 0.866_025_403_784_438_6).abs() < 1e-9);
    }

    #[test]
    fn ema_first_update_equals_input() {
        let mut ema = Ema::new(10);
        assert_eq!(ema.update(100.0), 100.0);
    }

    #[test]
    fn ema_converges_toward_constant_input() {
        let mut ema = Ema::new(5);
        ema.update(10.0);
        let mut last = 10.0;
        for _ in 0..50 {
            last = ema.update(20.0);
        }
        assert!((last - 20.0).abs() < 0.01);
    }

    #[test]
    fn wilder_average_none_before_period_then_seeds_as_simple_mean() {
        let mut w = WilderAverage::new(3);
        assert_eq!(w.update(1.0), None);
        assert_eq!(w.update(2.0), None);
        // (1+2+3)/3 = 2.0
        assert_eq!(w.update(3.0), Some(2.0));
    }

    #[test]
    fn wilder_average_smooths_after_seed() {
        let mut w = WilderAverage::new(3);
        w.update(1.0);
        w.update(2.0);
        w.update(3.0); // seed = 2.0
                       // next = 2.0 + (6.0 - 2.0)/3 = 3.333...
        let next = w.update(6.0).unwrap();
        assert!((next - 3.333_333_333_333_333).abs() < 1e-9);
    }

    #[test]
    fn ewma_variance_seeds_with_first_squared_return() {
        let mut v = EwmaVariance::new(0.94);
        assert_eq!(v.update(0.02), 0.0004);
    }

    #[test]
    fn ewma_variance_blends_toward_new_observations() {
        let mut v = EwmaVariance::new(0.5);
        v.update(1.0); // seeds variance = 1.0
        let next = v.update(0.0); // 0.5*1.0 + 0.5*0.0 = 0.5
        assert_eq!(next, 0.5);
    }
}
