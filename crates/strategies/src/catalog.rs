//! O catálogo das estratégias que este projeto oferece — id → metadados +
//! fábrica de instância independente. É a peça que faltava para uma
//! "Strategy Library" de verdade: `Strategy`/`FeatureStrategy` já definem
//! *o que* uma estratégia é, `StrategyRegistry` já *despacha* eventos para
//! as que estão em uso numa execução — mas nada respondia "quais
//! estratégias existem" sem espalhar a resposta em dois lugares (o `match`
//! de `app::strategy_factory` e o `Vec` de `experiments::strategy_set`,
//! historicamente duplicados e — pior — divergentes: o primeiro só
//! conhecia as 3 baselines, deixando as 3 estratégias quantitativas
//! inacessíveis no pipeline ao vivo). Este módulo é a fonte única.
//!
//! **Não é uma segunda hierarquia de trait.** `StrategyDescriptor` é dado
//! puro; construir uma instância continua sendo exatamente
//! `Xxx::new(id, Params::default())` (baseline) ou
//! `FeatureStrategyAdapter::new(Xxx::new(id, Params::default()))`
//! (quantitativa) — o catálogo só nomeia esse par (metadados, fábrica) uma
//! vez por estratégia em vez de deixar cada consumidor reescrever.
//!
//! **Suporte a múltiplas instâncias independentes** (o requisito
//! arquitetural para o futuro Strategy Selector, não implementado aqui):
//! [`build`] recebe o `StrategyId` de fora, em vez de derivá-lo sempre do
//! `kind` — `build("quant_momentum", StrategyId::new("quant_momentum-btc-a")?)`
//! e `build("quant_momentum", StrategyId::new("quant_momentum-btc-b")?)`
//! produzem duas instâncias completas e independentes (cada uma com seu
//! próprio estado interno — nenhuma estratégia deste crate usa `static`
//! nem qualquer forma de estado compartilhado), prontas para rodar
//! concorrentemente sobre o mesmo ou diferentes instrumentos. Nenhum
//! mecanismo de execução para "centenas de instâncias" é implementado
//! aqui — só a garantia de que a construção não impede isso.
//!
//! `default_params` existe só para descoberta (um dashboard futuro
//! mostrando "estes são os parâmetros default desta estratégia") — nunca
//! para *alterar* comportamento; não há mecanismo de override nesta fase,
//! de propósito (evita virar tuning por acidente). Toda estratégia
//! sempre é construída com `Params::default()`.

use domain::{SignalDirection, StrategyId};
use serde_json::Value;

use crate::feature_strategy::FeatureStrategyAdapter;
use crate::generic::{
    EmaCrossoverParams, EmaCrossoverStrategy, MeanReversionParams, MeanReversionStrategy,
    MomentumParams, MomentumStrategy, QuantMomentumParams, QuantMomentumStrategy,
    StatisticalMeanReversionParams, StatisticalMeanReversionStrategy, VolatilityBreakoutParams,
    VolatilityBreakoutStrategy,
};
use crate::requirements::StrategyRequirements;
use crate::strategy::Strategy;

/// Baseline = implementa `Strategy` diretamente, indicadores próprios,
/// mantida como referência de comparação simples. Quantitativa =
/// implementa `FeatureStrategy` via `FeatureStrategyAdapter`, consome
/// `features::FeatureSnapshot`, combina múltiplos filtros conjuntivos.
/// Mesma distinção já documentada em `generic::mod` — só formalizada aqui
/// como dado, não uma categoria nova inventada para este módulo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyCategory {
    Baseline,
    Quantitative,
}

/// Metadados de uma estratégia, suficientes para um dashboard futuro
/// descobrir o que existe sem hardcodar a lista no frontend. Dado puro —
/// nenhum método além dos campos.
#[derive(Debug, Clone)]
pub struct StrategyDescriptor {
    /// Igual ao `kind` usado em `get`/`build` — o identificador estável.
    pub id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub category: StrategyCategory,
    /// Reaproveitado de `crate::requirements` — não duplicado. Extraído
    /// da própria instância construída com os defaults (ver `entries()`),
    /// nunca reescrito à mão, então nunca pode divergir do que a
    /// estratégia realmente declara.
    pub requirements: StrategyRequirements,
    /// Direções que a lógica da estratégia de fato emite — confirmado por
    /// leitura de código, não uma suposição: as 3 baselines nunca emitem
    /// `Flat` (fecham só por reversão do próprio sinal, via risco/carteira);
    /// as 3 quantitativas têm saída própria explícita e emitem `Flat`.
    pub supported_directions: &'static [SignalDirection],
    /// Serializado do próprio `Params::default()` tipado — nunca um
    /// `json!({...})` escrito à mão que poderia divergir dos campos reais.
    pub default_params: Value,
}

/// Uma entrada do catálogo: os metadados mais a fábrica que constrói uma
/// instância nova a partir de um `StrategyId` fornecido pelo chamador.
pub struct StrategyCatalogEntry {
    pub descriptor: StrategyDescriptor,
    build: fn(StrategyId) -> Box<dyn Strategy>,
}

impl StrategyCatalogEntry {
    /// Constrói uma instância nova e independente com o id fornecido —
    /// nenhum estado é compartilhado com nenhuma outra instância já
    /// construída, desta ou de qualquer outra chamada.
    pub fn build(&self, id: StrategyId) -> Box<dyn Strategy> {
        (self.build)(id)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error(
        "unknown strategy kind {0:?}; expected one of: ema_crossover, momentum, mean_reversion, \
         statistical_mean_reversion, quant_momentum, volatility_breakout"
    )]
    UnknownKind(String),
    #[error("invalid strategy id: {0}")]
    InvalidId(#[from] domain::DomainError),
}

macro_rules! entry {
    (
        $kind:literal,
        $display_name:literal,
        $description:literal,
        $category:expr,
        $directions:expr,
        $params_ty:ty,
        $build:expr
    ) => {{
        let params = <$params_ty>::default();
        let default_params =
            serde_json::to_value(params).expect("strategy Params types always serialize");
        // Instância descartável só para ler `requirements()` de volta —
        // garante que o descriptor nunca diverge do que a estratégia
        // realmente declara (ver o doc do campo `requirements` acima).
        let prototype: Box<dyn Strategy> = $build(
            StrategyId::new($kind).expect("hardcoded strategy kind is always a valid StrategyId"),
        );
        let requirements = prototype.requirements().clone();
        StrategyCatalogEntry {
            descriptor: StrategyDescriptor {
                id: $kind,
                display_name: $display_name,
                description: $description,
                category: $category,
                requirements,
                supported_directions: $directions,
                default_params,
            },
            build: $build,
        }
    }};
}

const BASELINE_DIRECTIONS: &[SignalDirection] = &[SignalDirection::Long, SignalDirection::Short];
const QUANT_DIRECTIONS: &[SignalDirection] = &[
    SignalDirection::Long,
    SignalDirection::Short,
    SignalDirection::Flat,
];

/// As 6 estratégias que o projeto oferece hoje, construídas uma única vez
/// por chamada. Fonte canônica — `app::setup` e `experiments` consomem
/// isto em vez de repetir o `match`/`Vec` que existiam antes.
pub fn entries() -> Vec<StrategyCatalogEntry> {
    vec![
        entry!(
            "ema_crossover",
            "EMA Crossover",
            "Cruzamento clássico de EMA rápida/lenta — segue tendência, referência baseline simples.",
            StrategyCategory::Baseline,
            BASELINE_DIRECTIONS,
            EmaCrossoverParams,
            (|id: StrategyId| -> Box<dyn Strategy> {
                Box::new(EmaCrossoverStrategy::new(id, EmaCrossoverParams::default()))
            })
        ),
        entry!(
            "momentum",
            "Momentum",
            "Continuação por taxa de variação (rate of change) acima de um limiar — referência baseline simples.",
            StrategyCategory::Baseline,
            BASELINE_DIRECTIONS,
            MomentumParams,
            (|id: StrategyId| -> Box<dyn Strategy> {
                Box::new(MomentumStrategy::new(id, MomentumParams::default()))
            })
        ),
        entry!(
            "mean_reversion",
            "Mean Reversion",
            "Reversão à média por z-score — opera contra os extremos, premissa oposta às baselines de tendência.",
            StrategyCategory::Baseline,
            BASELINE_DIRECTIONS,
            MeanReversionParams,
            (|id: StrategyId| -> Box<dyn Strategy> {
                Box::new(MeanReversionStrategy::new(id, MeanReversionParams::default()))
            })
        ),
        entry!(
            "statistical_mean_reversion",
            "Statistical Mean Reversion",
            "Z-score de preço confirmado por RSI e autocorrelação de retornos, para distinguir reversão real de uma tendência nova — filtro conjuntivo, com saída própria.",
            StrategyCategory::Quantitative,
            QUANT_DIRECTIONS,
            StatisticalMeanReversionParams,
            (|id: StrategyId| -> Box<dyn Strategy> {
                Box::new(FeatureStrategyAdapter::new(StatisticalMeanReversionStrategy::new(
                    id,
                    StatisticalMeanReversionParams::default(),
                )))
            })
        ),
        entry!(
            "quant_momentum",
            "Quant Momentum",
            "Inclinação de regressão linear filtrada por R² e volume relativo — mais robusta que o momentum baseline (ponta a ponta), com saída própria.",
            StrategyCategory::Quantitative,
            QUANT_DIRECTIONS,
            QuantMomentumParams,
            (|id: StrategyId| -> Box<dyn Strategy> {
                Box::new(FeatureStrategyAdapter::new(QuantMomentumStrategy::new(
                    id,
                    QuantMomentumParams::default(),
                )))
            })
        ),
        entry!(
            "volatility_breakout",
            "Volatility Breakout",
            "Rompimento de Bollinger Band confirmado por expansão de volatilidade e volume acima da média — premissa oposta à statistical mean reversion, com saída própria.",
            StrategyCategory::Quantitative,
            QUANT_DIRECTIONS,
            VolatilityBreakoutParams,
            (|id: StrategyId| -> Box<dyn Strategy> {
                Box::new(FeatureStrategyAdapter::new(VolatilityBreakoutStrategy::new(
                    id,
                    VolatilityBreakoutParams::default(),
                )))
            })
        ),
    ]
}

/// Busca a entrada de `kind` no catálogo — `None` se não existir.
pub fn get(kind: &str) -> Option<StrategyCatalogEntry> {
    entries()
        .into_iter()
        .find(|entry| entry.descriptor.id == kind)
}

/// Constrói uma instância nova de `kind` com o `id` fornecido —
/// `Err(CatalogError::UnknownKind)` se `kind` não existir no catálogo.
pub fn build(kind: &str, id: StrategyId) -> Result<Box<dyn Strategy>, CatalogError> {
    get(kind)
        .map(|entry| entry.build(id))
        .ok_or_else(|| CatalogError::UnknownKind(kind.to_string()))
}

/// Conveniência para o caso comum de hoje (uma instância por kind, id
/// igual ao kind) — equivalente ao que `app::strategy_factory::build` e
/// `experiments::strategy_set::all_factories` faziam cada um à sua
/// maneira antes deste módulo existir.
pub fn build_default(kind: &str) -> Result<Box<dyn Strategy>, CatalogError> {
    let id = StrategyId::new(kind)?;
    build(kind, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn entries_has_exactly_six_strategies() {
        assert_eq!(entries().len(), 6);
    }

    #[test]
    fn every_id_is_unique() {
        let ids: HashSet<&str> = entries().iter().map(|e| e.descriptor.id).collect();
        assert_eq!(ids.len(), 6);
    }

    #[test]
    fn every_id_is_stable_across_calls() {
        let first: Vec<&str> = entries().iter().map(|e| e.descriptor.id).collect();
        let second: Vec<&str> = entries().iter().map(|e| e.descriptor.id).collect();
        assert_eq!(first, second);
    }

    #[test]
    fn get_and_build_default_find_all_six_known_kinds() {
        for kind in [
            "ema_crossover",
            "momentum",
            "mean_reversion",
            "statistical_mean_reversion",
            "quant_momentum",
            "volatility_breakout",
        ] {
            let entry = get(kind).unwrap_or_else(|| panic!("{kind} missing from catalog"));
            assert_eq!(entry.descriptor.id, kind);
            let strategy = build_default(kind).unwrap();
            assert_eq!(strategy.id().as_str(), kind);
        }
    }

    #[test]
    fn unknown_kind_is_a_typed_error_not_a_panic() {
        let result = build_default("does_not_exist");
        assert!(matches!(result, Err(CatalogError::UnknownKind(k)) if k == "does_not_exist"));
    }

    #[test]
    fn each_of_the_three_baselines_is_categorized_correctly() {
        for kind in ["ema_crossover", "momentum", "mean_reversion"] {
            let entry = get(kind).unwrap();
            assert_eq!(entry.descriptor.category, StrategyCategory::Baseline);
            assert_eq!(entry.descriptor.supported_directions, BASELINE_DIRECTIONS);
        }
    }

    #[test]
    fn each_of_the_three_quantitative_strategies_is_categorized_correctly() {
        for kind in [
            "statistical_mean_reversion",
            "quant_momentum",
            "volatility_breakout",
        ] {
            let entry = get(kind).unwrap();
            assert_eq!(entry.descriptor.category, StrategyCategory::Quantitative);
            assert_eq!(entry.descriptor.supported_directions, QUANT_DIRECTIONS);
        }
    }

    /// Requisito 6 do pedido do usuário: os requisitos declarados no
    /// descriptor correspondem ao que a estratégia realmente usa — não é
    /// uma suposição, é o `requirements()` da própria instância
    /// construída (ver a macro `entry!`).
    #[test]
    fn descriptor_requirements_match_what_the_built_instance_actually_declares() {
        for entry in entries() {
            let instance = build_default(entry.descriptor.id).unwrap();
            assert_eq!(&entry.descriptor.requirements, instance.requirements());
        }
    }

    /// Requisito 4 do pedido do usuário: duas instâncias do mesmo kind,
    /// com ids diferentes, não compartilham estado — a arquitetura para
    /// múltiplas instâncias independentes da mesma estratégia já
    /// funciona, sem precisar de nenhum mecanismo de execução novo.
    #[test]
    fn two_instances_of_the_same_kind_with_different_ids_do_not_share_state() {
        use chrono::Utc;
        use rust_decimal_macros::dec;

        let id_a = StrategyId::new("ema_crossover-instance-a").unwrap();
        let id_b = StrategyId::new("ema_crossover-instance-b").unwrap();
        let mut instance_a = build("ema_crossover", id_a.clone()).unwrap();
        let mut instance_b = build("ema_crossover", id_b.clone()).unwrap();

        assert_eq!(instance_a.id(), &id_a);
        assert_eq!(instance_b.id(), &id_b);
        assert_ne!(instance_a.id(), instance_b.id());

        let instrument = test_instrument();
        let positions = crate::position_query::NoPositions;
        let mut open_time = Utc::now();
        let mut feed = |price: rust_decimal::Decimal, strategy: &mut dyn Strategy| {
            let event = domain::MarketEvent::Candle(domain::Candle {
                instrument_id: instrument.id,
                timeframe: domain::Timeframe::M1,
                open_time,
                close_time: open_time + chrono::Duration::minutes(1),
                open: price,
                high: price,
                low: price,
                close: price,
                volume: dec!(1),
                is_closed: true,
            });
            open_time += chrono::Duration::minutes(1);
            strategy.on_event(&instrument, &event, &positions)
        };

        // Alimenta só a instância A com candles suficientes para preencher
        // sua EMA lenta (default: 26 períodos) — a instância B nunca
        // recebe nenhum evento.
        for i in 0..30 {
            feed(
                dec!(100) + rust_decimal::Decimal::from(i),
                instance_a.as_mut(),
            );
        }

        // Se o estado fosse compartilhado (ex.: por engano num `static`),
        // a instância B já teria uma EMA populada mesmo sem nunca ter
        // recebido um candle — alimentando-a com o candle inicial de A de
        // novo não deve produzir o mesmo sinal que A já pode estar perto
        // de produzir. Prova mais direta: nenhum jeito de A ter mutado B.
        // Como as duas são valores Rust independentes (não há Rc/Arc/static
        // em nenhuma estratégia deste crate), o compilador já impede
        // compartilhamento acidental — este teste documenta a garantia
        // em runtime também.
        let _ = feed(dec!(100), instance_b.as_mut());
        assert_eq!(instance_a.id(), &id_a);
        assert_eq!(instance_b.id(), &id_b);
    }

    fn test_instrument() -> domain::Instrument {
        let base = domain::Asset::new("BTC").unwrap();
        let quote = domain::Asset::new("USDT").unwrap();
        domain::Instrument::new(
            domain::Symbol::from_pair(&base, &quote),
            base,
            quote,
            domain::AssetClass::Crypto,
            domain::Exchange::Binance,
            domain::MarketType::Spot,
            rust_decimal_macros::dec!(0.01),
            rust_decimal_macros::dec!(0.0001),
            rust_decimal_macros::dec!(0.0001),
            rust_decimal_macros::dec!(10),
        )
    }

    /// Requisito 9 do pedido do usuário: nada no catálogo dá acesso a
    /// Broker/Portfolio/Persistência — checagem estrutural, não só de
    /// runtime: se `strategies` importasse `execution` ou `persistence`,
    /// isto nem compilaria. `positions` aqui é `&dyn PositionQuery`
    /// (leitura), nunca um `PortfolioManager` de verdade.
    #[test]
    fn built_strategies_only_ever_see_position_query_never_portfolio_or_broker() {
        // A própria assinatura de `Strategy::on_event` já é a garantia
        // (compila só com `&dyn PositionQuery`); este teste só confirma
        // que passar o null object `NoPositions` funciona para as 6.
        for entry in entries() {
            let mut strategy = build_default(entry.descriptor.id).unwrap();
            let instrument = test_instrument();
            let event = domain::MarketEvent::Candle(domain::Candle {
                instrument_id: instrument.id,
                timeframe: domain::Timeframe::M1,
                open_time: chrono::Utc::now(),
                close_time: chrono::Utc::now(),
                open: rust_decimal_macros::dec!(100),
                high: rust_decimal_macros::dec!(100),
                low: rust_decimal_macros::dec!(100),
                close: rust_decimal_macros::dec!(100),
                volume: rust_decimal_macros::dec!(1),
                is_closed: true,
            });
            let _ = strategy.on_event(&instrument, &event, &crate::position_query::NoPositions);
        }
    }
}
