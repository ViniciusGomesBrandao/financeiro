//! Compatibilidade regime × kind de estratégia.
//!
//! Scores heurísticos em `[0, 1]` — preferência relativa, não probabilidade.
//! Kinds alinhados ao catálogo (`strategies::catalog`).

use crate::regime::MarketRegime;

/// Extrai o kind do id de instância (`{robot}::{kind}`) ou devolve o id
/// inteiro quando não há separador (modo legado / testes).
pub fn strategy_kind_from_id(strategy_id: &str) -> &str {
    strategy_id
        .rsplit_once("::")
        .map(|(_, kind)| kind)
        .unwrap_or(strategy_id)
}

/// Score de afinidade do `kind` ao `regime` (0 = inadequada, 1 = preferida).
pub fn regime_affinity(regime: MarketRegime, kind: &str) -> f64 {
    match regime {
        MarketRegime::Trending => match kind {
            "momentum" | "quant_momentum" => 1.0,
            "ema_crossover" => 0.9,
            "volatility_breakout" => 0.45,
            "mean_reversion" | "statistical_mean_reversion" => 0.15,
            _ => 0.4,
        },
        MarketRegime::Ranging => match kind {
            "mean_reversion" | "statistical_mean_reversion" => 1.0,
            "ema_crossover" | "momentum" | "quant_momentum" => 0.2,
            "volatility_breakout" => 0.25,
            _ => 0.4,
        },
        MarketRegime::VolatilityExpansion => match kind {
            "volatility_breakout" => 1.0,
            "quant_momentum" | "momentum" => 0.55,
            "ema_crossover" => 0.4,
            "mean_reversion" | "statistical_mean_reversion" => 0.15,
            _ => 0.4,
        },
        MarketRegime::Uncertain => match kind {
            // Generalistas: em regime misto ainda cruzam / revertem com
            // alguma frequência. Breakout exige expansão clara — raro
            // exatamente quando a classificação é Uncertain; escolhê-lo
            // no bootstrap deixa o robô sem Long por muitas velas.
            "ema_crossover" => 0.75,
            "mean_reversion" | "statistical_mean_reversion" => 0.65,
            "momentum" | "quant_momentum" => 0.55,
            "volatility_breakout" => 0.25,
            _ => 0.4,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_kind_from_instance_id() {
        assert_eq!(strategy_kind_from_id("btc-15m::momentum"), "momentum");
        assert_eq!(strategy_kind_from_id("ema_crossover"), "ema_crossover");
    }

    #[test]
    fn trending_prefers_momentum_family() {
        assert!(
            regime_affinity(MarketRegime::Trending, "momentum")
                > regime_affinity(MarketRegime::Trending, "mean_reversion")
        );
    }

    #[test]
    fn ranging_prefers_mean_reversion_family() {
        assert!(
            regime_affinity(MarketRegime::Ranging, "statistical_mean_reversion")
                > regime_affinity(MarketRegime::Ranging, "quant_momentum")
        );
    }

    #[test]
    fn uncertain_prefers_generalists_over_breakout() {
        assert!(
            regime_affinity(MarketRegime::Uncertain, "ema_crossover")
                > regime_affinity(MarketRegime::Uncertain, "volatility_breakout")
        );
        assert!(
            regime_affinity(MarketRegime::Uncertain, "mean_reversion")
                > regime_affinity(MarketRegime::Uncertain, "volatility_breakout")
        );
    }
}
