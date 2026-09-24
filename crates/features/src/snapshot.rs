//! O resultado de alimentar um candle ao `FeatureEngine`: um valor (ou
//! `None`, se a janela relevante ainda não encheu) por feature.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

/// Bandas de Bollinger — banda central (SMA), superior e inferior
/// (`SMA ± k*stddev`), mais duas leituras derivadas úteis para uso direto
/// como feature: `percent_b` (posição do preço dentro das bandas, 0 = banda
/// inferior, 1 = banda superior — pode passar de [0,1] se o preço romper a
/// banda) e `bandwidth` (largura relativa das bandas, uma leitura comum de
/// contração/expansão de volatilidade).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BollingerBands {
    pub middle: f64,
    pub upper: f64,
    pub lower: f64,
    /// `(close - lower) / (upper - lower)`.
    pub percent_b: f64,
    /// `(upper - lower) / middle`.
    pub bandwidth: f64,
}

/// Resultado de uma regressão linear simples (`preço = a + b*índice`) sobre
/// a janela mais recente — ver `ohlcv::regression` para a fórmula e as
/// limitações.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RollingRegression {
    pub slope: f64,
    pub intercept: f64,
    /// Coeficiente de determinação, em `[0, 1]` (regressão com um único
    /// preditor, então equivale a `correlação(x, y)²`).
    pub r_squared: f64,
}

/// Todas as features calculadas para um candle, na forma como o motor as
/// via *naquele instante* — nunca revisado retroativamente por candles
/// futuros (ver a garantia de não-look-ahead no doc do crate). Cada campo é
/// `Option` porque a feature correspondente exige uma janela cheia de
/// histórico antes de produzir um primeiro valor; `None` significa "ainda
/// não há dado suficiente", nunca "zero" ou um valor inventado.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSnapshot {
    pub instrument_id: domain::InstrumentId,
    pub timestamp: DateTime<Utc>,
    pub close: f64,

    /// Retornos simples multi-horizonte: `período (em candles) -> retorno`.
    /// Só contém entradas para os períodos de `FeatureConfig::return_periods`
    /// que já têm histórico suficiente.
    pub returns: BTreeMap<usize, f64>,

    pub sma: Option<f64>,
    pub ema: Option<f64>,
    /// Desvio padrão do **preço** (nível), não de retornos.
    pub stddev: Option<f64>,
    /// Desvio padrão de retornos log — ver `ohlcv::volatility`.
    pub realized_volatility: Option<f64>,
    /// Volatilidade EWMA (RiskMetrics), já como desvio padrão (raiz da
    /// variância), não a variância crua.
    pub ewma_volatility: Option<f64>,
    /// `(preço - média_móvel) / desvio_padrão`, ambos sobre a mesma janela.
    pub zscore: Option<f64>,
    pub bollinger: Option<BollingerBands>,
    pub atr: Option<f64>,
    /// RSI de Wilder, em `[0, 100]`.
    pub rsi: Option<f64>,
    /// Rate of change (`%`) — ver `ohlcv::momentum`.
    pub roc: Option<f64>,
    pub regression: Option<RollingRegression>,
    /// Autocorrelação (Pearson) dos retornos com a defasagem configurada,
    /// em `[-1, 1]`.
    pub autocorrelation: Option<f64>,
    /// Volume do candle atual dividido pela média móvel de volume da
    /// janela (incluindo o candle atual) — `1.0` = volume na média,
    /// `> 1.0` = acima da média.
    pub relative_volume: Option<f64>,
}
