//! As 6 estratégias do experimento — 3 baselines + 3 quantitativas — cada
//! uma delegada a `strategies::catalog` (a fonte canônica de "quais
//! estratégias existem", introduzida na Fase 1 do Strategy Library).
//!
//! Este módulo existe só porque `run_single_experiment` (`runner.rs`)
//! precisa de uma fábrica no formato `fn() -> Box<dyn Strategy>` (um
//! ponteiro de função sem captura, não um closure genérico) — o catálogo
//! em si expõe `build(kind, id)`/`build_default(kind)`, que aceitam
//! `&str` em tempo de execução e por isso não cabem nesse tipo. Os 6
//! literais de `kind` abaixo são o único resíduo de duplicação que
//! restou: a lógica de construção (parâmetros default, `StrategyId`,
//! `FeatureStrategyAdapter`) não é mais repetida aqui — cada fábrica só
//! chama `strategies::catalog::build_default`.
//!
//! Cada entrada continua sendo uma *fábrica*, não uma instância pronta:
//! `runner::run_single_experiment` precisa construir uma instância nova e
//! sem estado a cada combinação (instrumento, timeframe), nunca
//! reaproveitar uma já usada — é isso que garante isolamento total entre
//! execuções (nenhum estado de indicador/posição vaza de uma combinação
//! para outra).

use strategies::Strategy;

pub type StrategyFactory = fn() -> Box<dyn Strategy>;

fn build(kind: &'static str) -> Box<dyn Strategy> {
    strategies::catalog::build_default(kind)
        .unwrap_or_else(|err| panic!("catalog entry for {kind:?} always exists: {err}"))
}

/// As 6 estratégias, em ordem estável (baselines primeiro, depois
/// quantitativas) — cada item é `(kind, factory)`, onde `kind` é também o
/// `StrategyId` que a estratégia construída carrega (mesmo comportamento
/// de antes: `build_default` usa `StrategyId::new(kind)`).
pub fn all_factories() -> Vec<(&'static str, StrategyFactory)> {
    vec![
        ("ema_crossover", || build("ema_crossover")),
        ("momentum", || build("momentum")),
        ("mean_reversion", || build("mean_reversion")),
        ("statistical_mean_reversion", || {
            build("statistical_mean_reversion")
        }),
        ("quant_momentum", || build("quant_momentum")),
        ("volatility_breakout", || build("volatility_breakout")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_exactly_six_strategies_with_stable_ids() {
        let factories = all_factories();
        assert_eq!(factories.len(), 6);
        for (kind, factory) in factories {
            let strategy = factory();
            assert_eq!(strategy.id().as_str(), kind);
        }
    }

    #[test]
    fn each_call_produces_a_fresh_stateless_instance() {
        // Duas chamadas da mesma fábrica não podem compartilhar estado —
        // cada `Box<dyn Strategy>` é uma alocação nova e independente.
        let factories = all_factories();
        let (_, factory) = &factories[0];
        let a = factory();
        let b = factory();
        assert!(!std::ptr::eq(
            a.as_ref() as *const dyn Strategy as *const (),
            b.as_ref() as *const dyn Strategy as *const ()
        ));
    }

    /// Confirma que este wrapper e `strategies::catalog` nunca podem
    /// divergir sobre quais 6 kinds existem — se alguém adicionar uma
    /// estratégia ao catálogo e esquecer de adicioná-la aqui (ou
    /// vice-versa), este teste falha.
    #[test]
    fn factory_kinds_match_the_catalog_exactly() {
        let mut factory_kinds: Vec<&str> = all_factories().into_iter().map(|(k, _)| k).collect();
        let mut catalog_kinds: Vec<&str> = strategies::catalog::entries()
            .into_iter()
            .map(|e| e.descriptor.id)
            .collect();
        factory_kinds.sort_unstable();
        catalog_kinds.sort_unstable();
        assert_eq!(factory_kinds, catalog_kinds);
    }
}
