//! Três leituras de dispersão/volatilidade — deliberadamente separadas
//! porque respondem perguntas diferentes:
//!
//! - **Desvio padrão do preço** (`PriceStdDev`): dispersão do *nível* de
//!   preço na janela — a mesma estatística usada pelas Bollinger Bands.
//!   Não é comparável entre instrumentos de preços muito diferentes (BTC
//!   vs. uma moeda de USD 0,01), porque não é normalizada pelo preço.
//! - **Volatilidade realizada** (`RealizedVolatility`): desvio padrão dos
//!   *retornos log*, a medida padrão de volatilidade em finanças — já
//!   comparável entre instrumentos, porque retorno é adimensional.
//!   `retorno_log_t = ln(close_t / close_{t-1})`.
//! - **Volatilidade EWMA** (`EwmaVolatility`): como a realizada, mas com
//!   peso exponencialmente maior para retornos recentes (estilo
//!   RiskMetrics) em vez de peso igual dentro de uma janela fixa — reage
//!   mais rápido a um choque de volatilidade, ao custo de "esquecer" mais
//!   devagar (o peso de um choque antigo decai geometricamente, nunca cai a
//!   zero de fato).
//!
//! **Limitações comuns às três**: nenhuma é anualizada aqui — o valor é
//! "por barra", na unidade de tempo do candle alimentado (ex.: volatilidade
//! de 1 minuto, se o motor recebe candles de 1m). Anualizar exigiria saber
//! quantas barras existem por ano, que varia por timeframe/mercado
//! (cripto negocia 24/7, ações não) — decisão deixada para quem consome a
//! feature, não hardcoded aqui.

use crate::primitives::{EwmaVariance, RollingWindow};

#[derive(Debug, Clone)]
pub struct PriceStdDev {
    window: RollingWindow,
}

impl PriceStdDev {
    pub fn new(period: usize) -> Self {
        Self {
            window: RollingWindow::new(period),
        }
    }

    pub fn update(&mut self, close: f64) -> Option<f64> {
        self.window.push(close);
        if self.window.is_full() {
            self.window.stddev()
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct RealizedVolatility {
    window: RollingWindow,
    prev_close: Option<f64>,
}

impl RealizedVolatility {
    pub fn new(period: usize) -> Self {
        Self {
            window: RollingWindow::new(period),
            prev_close: None,
        }
    }

    /// `None` enquanto não houver `period` retornos log acumulados (o que
    /// exige `period + 1` candles, já que o primeiro candle não produz
    /// retorno algum).
    pub fn update(&mut self, close: f64) -> Option<f64> {
        let result = match self.prev_close {
            Some(prev) if prev > 0.0 && close > 0.0 => {
                let log_return = (close / prev).ln();
                self.window.push(log_return);
                if self.window.is_full() {
                    self.window.stddev()
                } else {
                    None
                }
            }
            _ => None,
        };
        self.prev_close = Some(close);
        result
    }
}

#[derive(Debug, Clone)]
pub struct EwmaVolatility {
    variance: EwmaVariance,
    prev_close: Option<f64>,
}

impl EwmaVolatility {
    pub fn new(lambda: f64) -> Self {
        Self {
            variance: EwmaVariance::new(lambda),
            prev_close: None,
        }
    }

    /// `None` no primeiro candle (não há retorno ainda); a partir do
    /// segundo, sempre produz um valor — mas o primeiro valor produzido
    /// carrega a limitação de "sem burn-in" documentada em
    /// `primitives::EwmaVariance`.
    pub fn update(&mut self, close: f64) -> Option<f64> {
        let result = match self.prev_close {
            Some(prev) if prev > 0.0 && close > 0.0 => {
                let log_return = (close / prev).ln();
                Some(self.variance.update(log_return).sqrt())
            }
            _ => None,
        };
        self.prev_close = Some(close);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_stddev_matches_rolling_window() {
        let mut sd = PriceStdDev::new(4);
        for v in [2.0, 4.0, 4.0, 4.0] {
            sd.update(v);
        }
        assert!((sd.update(4.0).unwrap() - 0.0).abs() < 1e-12); // janela agora [4,4,4,4]
    }

    #[test]
    fn realized_volatility_needs_period_plus_one_candles() {
        let mut rv = RealizedVolatility::new(2);
        assert_eq!(rv.update(100.0), None); // sem retorno ainda
        assert_eq!(rv.update(110.0), None); // 1 retorno, período=2 exige 2
        let out = rv.update(100.0);
        assert!(out.is_some());
    }

    #[test]
    fn realized_volatility_zero_for_constant_returns() {
        // Retorno log constante em cada passo -> desvio padrão zero.
        let mut rv = RealizedVolatility::new(2);
        rv.update(100.0);
        rv.update(110.0);
        let v = rv.update(121.0).unwrap(); // dois retornos log iguais: ln(1.1)
        assert!(v.abs() < 1e-9);
    }

    #[test]
    fn ewma_volatility_none_on_first_candle_then_seeds() {
        let mut ev = EwmaVolatility::new(0.5);
        assert_eq!(ev.update(100.0), None);
        // retorno log = ln(110/100); variância semeada = retorno^2; vol = |retorno|
        let vol = ev.update(110.0).unwrap();
        let expected = (110.0_f64 / 100.0).ln().abs();
        assert!((vol - expected).abs() < 1e-12);
    }
}
