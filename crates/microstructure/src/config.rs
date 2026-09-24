//! Configuração do `MicrostructureFeatureEngine` — mesmo padrão de
//! `features::config::FeatureConfig`: um struct simples com `Default`
//! documentado, valores redondos e convencionais, **não** ajustados a
//! nenhum resultado de estratégia (não existe estratégia nesta fase).

/// - `depth_levels`: quantos níveis de cada lado do book entram no cálculo
///   de "order book imbalance" (em oposição ao imbalance L1, que sempre usa
///   só o melhor nível). 10 é um valor comum na literatura de
///   microestrutura para um primeiro corte — não foi escolhido observando
///   nenhum resultado.
/// - `trade_window_secs`: janela rolante (por tempo, não por contagem de
///   trades) usada por buy/sell trade imbalance, volume delta e trade
///   intensity — todas as três compartilham a mesma janela de trades
///   recentes. 30s é uma janela curta o bastante para refletir pressão de
///   curto prazo e longa o bastante para não ficar vazia em instrumentos de
///   menor liquidez.
/// - `ofi_window_secs`: janela rolante separada para o order-flow
///   imbalance acumulado — mantida distinta de `trade_window_secs` porque
///   OFI é derivado de transições do book (não de trades) e pode fazer
///   sentido observar em outra escala de tempo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MicrostructureConfig {
    pub depth_levels: usize,
    pub trade_window_secs: u64,
    pub ofi_window_secs: u64,
}

impl Default for MicrostructureConfig {
    fn default() -> Self {
        Self {
            depth_levels: 10,
            trade_window_secs: 30,
            ofi_window_secs: 30,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_uses_documented_round_values() {
        let config = MicrostructureConfig::default();
        assert_eq!(config.depth_levels, 10);
        assert_eq!(config.trade_window_secs, 30);
        assert_eq!(config.ofi_window_secs, 30);
    }
}
