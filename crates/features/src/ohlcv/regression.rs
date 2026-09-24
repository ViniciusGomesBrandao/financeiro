//! Regressão linear rolling (slope + R²) do preço de fechamento contra o
//! tempo (índice do candle dentro da janela).
//!
//! **Significado**: `slope` é a taxa de variação do preço por candle — uma
//! medida de tendência mais suave que comparar só dois pontos (como o
//! momentum faz). `R²` mede o quão bem essa tendência linear explica o
//! movimento observado: perto de 1 = tendência limpa/consistente; perto de
//! 0 = preço lateral ou serrilhado, mesmo que o `slope` não seja zero.
//!
//! **Fórmula** (mínimos quadrados ordinários, `x = 0..N-1`, `y = close`):
//! `slope = Cov(x,y) / Var(x)`; `intercept = média(y) - slope*média(x)`;
//! `R² = 1 - SS_res/SS_tot`, onde `SS_res = Σ(y_i - (intercept+slope*x_i))²`
//! e `SS_tot = Σ(y_i - média(y))²`. Como há só um preditor, isso equivale a
//! `R² = correlação(x,y)²`, mas a fórmula acima é calculada diretamente,
//! sem depender dessa equivalência.
//!
//! **Limitações**: `x` é o índice do candle (0, 1, 2, ...), não tempo de
//! calendário — só é válido como "taxa por candle" quando os candles são
//! aproximadamente equiespaçados no tempo, o que candles de mesmo
//! timeframe normalmente são. `R²` fica indefinido (`None`) quando
//! `SS_tot == 0` (preço constante na janela inteira).

use crate::primitives::RollingWindow;
use crate::snapshot::RollingRegression;

#[derive(Debug, Clone)]
pub struct RollingLinearRegression {
    window: RollingWindow,
}

impl RollingLinearRegression {
    pub fn new(period: usize) -> Self {
        assert!(period >= 2, "regression window must have at least 2 points");
        Self {
            window: RollingWindow::new(period),
        }
    }

    pub fn update(&mut self, close: f64) -> Option<RollingRegression> {
        self.window.push(close);
        if !self.window.is_full() {
            return None;
        }
        let ys = self.window.to_vec();
        let n = ys.len() as f64;
        let mean_x = (ys.len() as f64 - 1.0) / 2.0;
        let mean_y = ys.iter().sum::<f64>() / n;

        let mut sxy = 0.0;
        let mut sxx = 0.0;
        let mut ss_tot = 0.0;
        for (i, &y) in ys.iter().enumerate() {
            let x = i as f64;
            sxy += (x - mean_x) * (y - mean_y);
            sxx += (x - mean_x).powi(2);
            ss_tot += (y - mean_y).powi(2);
        }

        if sxx == 0.0 {
            return None;
        }
        let slope = sxy / sxx;
        let intercept = mean_y - slope * mean_x;

        if ss_tot == 0.0 {
            // Janela perfeitamente constante: slope é 0/sxx = 0, mas R² é
            // indefinido (0/0), não 1 nem 0 — reportado como None em vez
            // de escolher um valor arbitrário.
            return None;
        }

        let mut ss_res = 0.0;
        for (i, &y) in ys.iter().enumerate() {
            let predicted = intercept + slope * i as f64;
            ss_res += (y - predicted).powi(2);
        }
        let r_squared = 1.0 - ss_res / ss_tot;

        Some(RollingRegression {
            slope,
            intercept,
            r_squared,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_uptrend_has_r_squared_one() {
        let mut reg = RollingLinearRegression::new(4);
        for v in [10.0, 12.0, 14.0, 16.0] {
            reg.update(v);
        }
        let out = reg.update(18.0).unwrap(); // janela [12,14,16,18], slope=2
        assert!((out.slope - 2.0).abs() < 1e-9);
        assert!((out.r_squared - 1.0).abs() < 1e-9);
    }

    #[test]
    fn flat_window_returns_none() {
        let mut reg = RollingLinearRegression::new(3);
        for v in [5.0, 5.0, 5.0] {
            reg.update(v);
        }
        assert_eq!(reg.update(5.0), None);
    }

    #[test]
    fn noisy_series_has_r_squared_between_zero_and_one() {
        let mut reg = RollingLinearRegression::new(5);
        for v in [10.0, 11.0, 9.0, 12.0, 8.0] {
            reg.update(v);
        }
        let out = reg.update(13.0).unwrap();
        assert!(out.r_squared >= 0.0 && out.r_squared <= 1.0);
    }
}
