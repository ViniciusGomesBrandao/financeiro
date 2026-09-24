//! Classificação determinística do regime de mercado a partir de
//! `features::FeatureSnapshot` — reutiliza limiares já usados pelas
//! estratégias quantitativas do catálogo, em vez de inventar novos.

use features::FeatureSnapshot;

/// Regimes suportados nesta fase. Conjunto mínimo útil e explicável.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarketRegime {
    /// Tendência limpa (regressão forte + autocorrelação de continuação).
    Trending,
    /// Lateralização / ausência de tendência persistente.
    Ranging,
    /// Rompimento com expansão de volatilidade e volume.
    VolatilityExpansion,
    /// Features insuficientes ou sinais contraditórios.
    Uncertain,
}

impl MarketRegime {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Trending => "trending",
            Self::Ranging => "ranging",
            Self::VolatilityExpansion => "volatility_expansion",
            Self::Uncertain => "uncertain",
        }
    }
}

/// Evidência numérica usada na classificação (para observabilidade).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegimeEvidence {
    pub r_squared: Option<f64>,
    pub slope: Option<f64>,
    pub autocorrelation: Option<f64>,
    pub bandwidth: Option<f64>,
    pub prev_bandwidth: Option<f64>,
    pub relative_volume: Option<f64>,
    pub percent_b: Option<f64>,
    pub zscore: Option<f64>,
}

/// Resultado da classificação: regime + força heurística (não probabilidade).
#[derive(Debug, Clone, PartialEq)]
pub struct RegimeAssessment {
    pub regime: MarketRegime,
    /// Força heurística da classificação em `[0, 1]` — score de decisão,
    /// nunca probabilidade calibrada.
    pub strength: f64,
    /// Código estável da regra que disparou (ex.: `trend_r2_and_autocorr`).
    pub rule: &'static str,
    /// Frase curta e explicável para UI/logs.
    pub summary: String,
    pub evidence: RegimeEvidence,
}

/// Limiares alinhados aos defaults das estratégias:
/// - `min_r_squared` = `QuantMomentumParams::default` (0.6)
/// - `max_autocorrelation` = `StatisticalMeanReversionParams::default` (0.3)
/// - `min_relative_volume` = `VolatilityBreakoutParams::default` (1.5)
/// - `ranging_r_squared` = abaixo de metade do limiar de tendência limpa
pub struct RegimeThresholds {
    pub min_r_squared: f64,
    pub trend_autocorr: f64,
    pub ranging_r_squared: f64,
    pub min_relative_volume: f64,
}

impl Default for RegimeThresholds {
    fn default() -> Self {
        Self {
            min_r_squared: 0.6,
            trend_autocorr: 0.3,
            ranging_r_squared: 0.3,
            min_relative_volume: 1.5,
        }
    }
}

/// Classifica o regime no instante do snapshot.
///
/// `prev_bandwidth` é o bandwidth do candle anterior (já conhecido), nunca
/// de um candle futuro — o chamador deve alimentar em ordem cronológica.
pub fn classify_regime(
    snapshot: &FeatureSnapshot,
    prev_bandwidth: Option<f64>,
    thresholds: &RegimeThresholds,
) -> RegimeAssessment {
    let evidence = RegimeEvidence {
        r_squared: snapshot.regression.map(|r| r.r_squared),
        slope: snapshot.regression.map(|r| r.slope),
        autocorrelation: snapshot.autocorrelation,
        bandwidth: snapshot.bollinger.map(|b| b.bandwidth),
        prev_bandwidth,
        relative_volume: snapshot.relative_volume,
        percent_b: snapshot.bollinger.map(|b| b.percent_b),
        zscore: snapshot.zscore,
    };

    // 1) Breakout / expansão — regra mais específica primeiro.
    if let (Some(percent_b), Some(bw), Some(prev_bw), Some(rel_vol)) = (
        evidence.percent_b,
        evidence.bandwidth,
        evidence.prev_bandwidth,
        evidence.relative_volume,
    ) {
        let broke_out = !(0.0..=1.0).contains(&percent_b);
        let expanding = bw > prev_bw;
        if broke_out && expanding && rel_vol >= thresholds.min_relative_volume {
            let excess = if percent_b > 1.0 {
                percent_b - 1.0
            } else {
                -percent_b
            };
            let strength =
                (0.55 + excess * 0.3 + (rel_vol - 1.0).clamp(0.0, 1.0) * 0.15).clamp(0.0, 1.0);
            return RegimeAssessment {
                regime: MarketRegime::VolatilityExpansion,
                strength,
                rule: "breakout_expanding_bandwidth_volume",
                summary: format!(
                    "rompimento de banda com volatilidade em expansão e volume relativo {rel_vol:.2}"
                ),
                evidence,
            };
        }
    }

    // 2) Tendência limpa — mesmos limiares de quant_momentum + filtro SMR.
    if let (Some(r2), Some(autocorr), Some(slope)) =
        (evidence.r_squared, evidence.autocorrelation, evidence.slope)
    {
        if r2 >= thresholds.min_r_squared && autocorr > thresholds.trend_autocorr {
            let strength = ((r2 - thresholds.min_r_squared) / (1.0 - thresholds.min_r_squared)
                * 0.5
                + (autocorr - thresholds.trend_autocorr).clamp(0.0, 0.7) / 0.7 * 0.5)
                .clamp(0.0, 1.0);
            let direction = if slope >= 0.0 { "alta" } else { "baixa" };
            return RegimeAssessment {
                regime: MarketRegime::Trending,
                strength: strength.max(0.55),
                rule: "trend_r2_and_autocorr",
                summary: format!(
                    "tendência de {direction} com R²={r2:.2} e autocorrelação={autocorr:.2}"
                ),
                evidence,
            };
        }
    }

    // 3) Lateralização — tendência fraca e sem persistência de retornos.
    if let (Some(r2), Some(autocorr)) = (evidence.r_squared, evidence.autocorrelation) {
        if r2 <= thresholds.ranging_r_squared && autocorr <= thresholds.trend_autocorr {
            let strength = (1.0 - r2).clamp(0.4, 0.95);
            return RegimeAssessment {
                regime: MarketRegime::Ranging,
                strength,
                rule: "low_r2_and_low_autocorr",
                summary: format!("mercado lateralizado (R²={r2:.2}, autocorrelação={autocorr:.2})"),
                evidence,
            };
        }
    }

    // Fallback ranging só com R² baixo quando autocorr ainda não existe.
    if let Some(r2) = evidence.r_squared {
        if r2 <= thresholds.ranging_r_squared && evidence.autocorrelation.is_none() {
            return RegimeAssessment {
                regime: MarketRegime::Ranging,
                strength: (1.0 - r2).clamp(0.35, 0.8),
                rule: "low_r2_only",
                summary: format!("movimento pouco explicado pela tendência (R²={r2:.2})"),
                evidence,
            };
        }
    }

    let summary = if evidence.r_squared.is_none()
        && evidence.bandwidth.is_none()
        && evidence.autocorrelation.is_none()
    {
        "histórico insuficiente para classificar o regime".to_string()
    } else {
        "sinais de mercado mistos ou inconclusivos".to_string()
    };

    RegimeAssessment {
        regime: MarketRegime::Uncertain,
        strength: 0.0,
        rule: "uncertain",
        summary,
        evidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use domain::InstrumentId;
    use features::{BollingerBands, RollingRegression};
    use std::collections::BTreeMap;

    fn base_snap() -> FeatureSnapshot {
        FeatureSnapshot {
            instrument_id: InstrumentId::new(),
            timestamp: Utc::now(),
            close: 100.0,
            returns: BTreeMap::new(),
            sma: Some(100.0),
            ema: Some(100.0),
            stddev: Some(1.0),
            realized_volatility: Some(0.01),
            ewma_volatility: Some(0.01),
            zscore: Some(0.1),
            bollinger: None,
            atr: Some(1.0),
            rsi: Some(50.0),
            roc: Some(0.0),
            regression: None,
            autocorrelation: None,
            relative_volume: Some(1.0),
        }
    }

    #[test]
    fn classifies_clear_trend() {
        let mut snap = base_snap();
        snap.regression = Some(RollingRegression {
            slope: 0.5,
            intercept: 90.0,
            r_squared: 0.85,
        });
        snap.autocorrelation = Some(0.45);
        let a = classify_regime(&snap, None, &RegimeThresholds::default());
        assert_eq!(a.regime, MarketRegime::Trending);
        assert!(a.strength > 0.5);
        assert_eq!(a.rule, "trend_r2_and_autocorr");
    }

    #[test]
    fn classifies_clear_range() {
        let mut snap = base_snap();
        snap.regression = Some(RollingRegression {
            slope: 0.01,
            intercept: 100.0,
            r_squared: 0.15,
        });
        snap.autocorrelation = Some(0.05);
        let a = classify_regime(&snap, None, &RegimeThresholds::default());
        assert_eq!(a.regime, MarketRegime::Ranging);
        assert_eq!(a.rule, "low_r2_and_low_autocorr");
    }

    #[test]
    fn classifies_volatility_expansion_breakout() {
        let mut snap = base_snap();
        snap.bollinger = Some(BollingerBands {
            middle: 100.0,
            upper: 102.0,
            lower: 98.0,
            percent_b: 1.2,
            bandwidth: 0.08,
        });
        snap.relative_volume = Some(1.8);
        let a = classify_regime(&snap, Some(0.04), &RegimeThresholds::default());
        assert_eq!(a.regime, MarketRegime::VolatilityExpansion);
        assert_eq!(a.rule, "breakout_expanding_bandwidth_volume");
    }

    #[test]
    fn insufficient_features_are_uncertain() {
        let snap = base_snap();
        let a = classify_regime(&snap, None, &RegimeThresholds::default());
        assert_eq!(a.regime, MarketRegime::Uncertain);
    }

    #[test]
    fn future_bandwidth_is_never_read_here() {
        // O classificador só vê `prev_bandwidth` passado pelo caller —
        // não há look-ahead interno.
        let mut snap = base_snap();
        snap.bollinger = Some(BollingerBands {
            middle: 100.0,
            upper: 102.0,
            lower: 98.0,
            percent_b: 1.2,
            bandwidth: 0.08,
        });
        snap.relative_volume = Some(2.0);
        let without_prev = classify_regime(&snap, None, &RegimeThresholds::default());
        assert_ne!(without_prev.regime, MarketRegime::VolatilityExpansion);
    }
}
