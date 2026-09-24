//! Parâmetros (janelas/períodos) de cada feature. Um único `FeatureConfig`
//! parametriza um `FeatureEngine` inteiro — trocar um período não exige
//! tocar em código, só nesta struct.

/// Períodos e parâmetros de todas as features de `ohlcv`. Os defaults
/// (`Default`) são escolhas convencionais de análise técnica (RSI/ATR de
/// 14, Bollinger de 20 com k=2, lambda de EWMA de 0.94 no estilo
/// RiskMetrics), não parâmetros otimizados para nenhuma estratégia
/// específica — ajuste conforme o caso de uso.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureConfig {
    /// Horizontes (em número de candles) para os retornos multi-timeframe —
    /// ver o aviso de escopo em `ohlcv::returns`: são múltiplos
    /// *lookbacks* sobre a mesma série, não candles reamostrados em
    /// timeframes diferentes.
    pub return_periods: Vec<usize>,
    pub sma_period: usize,
    pub ema_period: usize,
    /// Período da janela de desvio padrão do **preço** (nível), usada
    /// também pelas Bollinger Bands.
    pub stddev_period: usize,
    /// Período da janela de volatilidade realizada (desvio padrão de
    /// *retornos* log, não de preço).
    pub realized_volatility_period: usize,
    /// Fator de decaimento da EWMA de volatilidade, em `[0, 1)`. `0.94` é o
    /// default popularizado pelo RiskMetrics para dados diários.
    pub ewma_lambda: f64,
    pub zscore_period: usize,
    pub bollinger_period: usize,
    /// Número de desvios padrão para as bandas superior/inferior.
    pub bollinger_k: f64,
    pub atr_period: usize,
    pub rsi_period: usize,
    /// Período do momentum/ROC (rate of change).
    pub roc_period: usize,
    pub regression_period: usize,
    /// Tamanho da janela de retornos para a autocorrelação.
    pub autocorrelation_period: usize,
    /// Defasagem (lag), em candles, da autocorrelação.
    pub autocorrelation_lag: usize,
    pub relative_volume_period: usize,
}

impl Default for FeatureConfig {
    fn default() -> Self {
        Self {
            return_periods: vec![1, 5, 15, 60],
            sma_period: 20,
            ema_period: 20,
            stddev_period: 20,
            realized_volatility_period: 20,
            ewma_lambda: 0.94,
            zscore_period: 20,
            bollinger_period: 20,
            bollinger_k: 2.0,
            atr_period: 14,
            rsi_period: 14,
            roc_period: 12,
            regression_period: 20,
            autocorrelation_period: 20,
            autocorrelation_lag: 1,
            relative_volume_period: 20,
        }
    }
}
