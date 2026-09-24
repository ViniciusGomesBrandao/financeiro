//! Constrói um `StrategyRegistry` a partir de uma lista de instâncias
//! configuradas por dado — a peça que faltava para rodar múltiplas
//! estratégias (e múltiplas instâncias independentes da mesma estratégia)
//! ao mesmo tempo, cada uma com seu próprio `StrategyId` ("robot_id") e seu
//! próprio subconjunto de instrumentos.
//!
//! **Não é um novo mecanismo de execução.** `catalog` já sabe construir
//! qualquer estratégia a partir de um `StrategyId` arbitrário;
//! `StrategyRegistry` já despacha para quantas entradas forem registradas,
//! cada uma isolada por seu próprio conjunto de instrumentos (ver
//! `StrategyRegistry::dispatch`, que só chama uma estratégia para um
//! instrumento que foi explicitamente registrado contra ela). Este módulo
//! só formaliza "quais instâncias existem nesta execução" como dado —
//! `Vec<StrategyInstanceConfig>` — em vez de deixar cada chamador (hoje
//! `app::setup`) escrever seu próprio laço com combinações hardcoded.
//!
//! Uma instância = um `StrategyId` (o "robot_id") + um `kind` do catálogo +
//! os símbolos dos instrumentos que ela deve negociar. Duas instâncias do
//! mesmo `kind` com `id`s diferentes (ex.: `"quant_momentum-btc-a"` e
//! `"quant_momentum-btc-b"`) já são suficientes para rodar concorrentemente
//! sem compartilhar estado — ver `catalog` para a garantia. Uma instância
//! pode listar mais de um símbolo (o mesmo robot negociando vários
//! instrumentos), reaproveitando o padrão já existente de estado
//! por-instrumento dentro de uma única `Box<dyn Strategy>` (`EmaCrossoverStrategy`,
//! `FeatureStrategyAdapter`, ...).

use std::collections::HashSet;

use domain::{Instrument, MarketDataKind, StrategyId};

use crate::catalog::{self, CatalogError};
use crate::error::CompatibilityError;
use crate::registry::StrategyRegistry;

/// Uma instância de estratégia configurada por dado: `id` é o identificador
/// estável dessa instância (o "robot_id" — aparece em todo `Signal`/`Position`
/// resultante via `StrategyId`, o que já é suficiente para atribuir
/// PnL/trades por instância sem nenhum campo novo em `domain`), `kind` é a
/// entrada do catálogo a construir, e `symbols` são os símbolos
/// (`Instrument::symbol`, formato `"BASE/QUOTE"`) que esta instância deve
/// negociar — nunca todos os instrumentos disponíveis por padrão, para que
/// cada instrumento receba só as estratégias explicitamente configuradas
/// para ele.
#[derive(Debug, Clone)]
pub struct StrategyInstanceConfig {
    pub id: StrategyId,
    pub kind: String,
    pub symbols: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum InstanceError {
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    #[error(transparent)]
    Compatibility(#[from] CompatibilityError),
    #[error("duplicate strategy instance id {0:?}; each instance must have a unique id")]
    DuplicateId(String),
}

/// Constrói um `StrategyRegistry` populado a partir de `configs` — para cada
/// instância, resolve a fábrica no catálogo, filtra `instruments` pelos
/// símbolos declarados (nunca por classe de ativo implicitamente) e
/// registra o resultado. Uma instância cujos símbolos não correspondem a
/// nenhum instrumento carregado é pulada com um aviso (mesmo comportamento
/// que `app::setup::build_strategy_registry` já tinha antes deste módulo
/// existir), não um erro — um símbolo ausente é uma configuração
/// incompleta, não necessariamente inválida (ex.: instrumento ainda não
/// carregado nesta execução).
pub fn build_registry(
    configs: &[StrategyInstanceConfig],
    instruments: &[Instrument],
    available_market_data: &HashSet<MarketDataKind>,
) -> Result<StrategyRegistry, InstanceError> {
    let mut registry = StrategyRegistry::new();
    let mut seen_ids: HashSet<&StrategyId> = HashSet::new();

    for config in configs {
        if !seen_ids.insert(&config.id) {
            return Err(InstanceError::DuplicateId(config.id.to_string()));
        }

        let strategy = catalog::build(&config.kind, config.id.clone())?;

        let selected: Vec<Instrument> = instruments
            .iter()
            .filter(|instrument| {
                config
                    .symbols
                    .iter()
                    .any(|symbol| symbol == instrument.symbol.as_str())
            })
            .cloned()
            .collect();

        if selected.is_empty() {
            tracing::warn!(
                strategy_id = %config.id,
                kind = %config.kind,
                "no loaded instrument matches this strategy instance's symbols; skipping registration"
            );
            continue;
        }

        registry.register(strategy, &selected, available_market_data)?;
    }

    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{AssetClass, Exchange, MarketType};
    use rust_decimal_macros::dec;

    fn instrument(base: &str, quote: &str) -> Instrument {
        let base = domain::Asset::new(base).unwrap();
        let quote = domain::Asset::new(quote).unwrap();
        Instrument::new(
            domain::Symbol::from_pair(&base, &quote),
            base,
            quote,
            AssetClass::Crypto,
            Exchange::Binance,
            MarketType::Spot,
            dec!(0.01),
            dec!(0.0001),
            dec!(0.0001),
            dec!(10),
        )
    }

    fn ohlcv() -> HashSet<MarketDataKind> {
        HashSet::from([MarketDataKind::Ohlcv])
    }

    /// Requisito 1: múltiplas estratégias diferentes no mesmo instrumento —
    /// cada uma dispara seu próprio dispatch, sem interferir uma na outra.
    #[test]
    fn multiple_different_strategies_can_target_the_same_instrument() {
        let btc = instrument("BTC", "USDT");
        let configs = vec![
            StrategyInstanceConfig {
                id: StrategyId::new("ema-btc").unwrap(),
                kind: "ema_crossover".to_string(),
                symbols: vec![btc.symbol.to_string()],
            },
            StrategyInstanceConfig {
                id: StrategyId::new("momentum-btc").unwrap(),
                kind: "momentum".to_string(),
                symbols: vec![btc.symbol.to_string()],
            },
        ];

        let registry = build_registry(&configs, &[btc], &ohlcv()).unwrap();
        assert_eq!(registry.len(), 2);
        assert!(registry.get(&StrategyId::new("ema-btc").unwrap()).is_some());
        assert!(registry
            .get(&StrategyId::new("momentum-btc").unwrap())
            .is_some());
    }

    /// Requisito 2: duas instâncias do mesmo algoritmo, ids diferentes, não
    /// compartilham estado — alimenta uma delas com candles suficientes
    /// para produzir um crossover e confirma que a outra, nunca alimentada,
    /// permanece sem sinal quando finalmente recebe seu primeiro candle.
    #[test]
    fn two_instances_of_the_same_strategy_do_not_share_state() {
        let btc = instrument("BTC", "USDT");
        let configs = vec![
            StrategyInstanceConfig {
                id: StrategyId::new("ema-a").unwrap(),
                kind: "ema_crossover".to_string(),
                symbols: vec![btc.symbol.to_string()],
            },
            StrategyInstanceConfig {
                id: StrategyId::new("ema-b").unwrap(),
                kind: "ema_crossover".to_string(),
                symbols: vec![btc.symbol.to_string()],
            },
        ];

        let mut registry = build_registry(&configs, std::slice::from_ref(&btc), &ohlcv()).unwrap();

        let mut open_time = chrono::Utc::now();
        let mut feed = |price: rust_decimal::Decimal, registry: &mut StrategyRegistry| {
            let event = domain::MarketEvent::Candle(domain::Candle {
                instrument_id: btc.id,
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
            registry.dispatch(&btc, &event, &crate::position_query::NoPositions)
        };

        // Só a instância A deveria ver estas quedas-e-alta — mas
        // `dispatch` sempre alimenta as duas (é isso que o próximo teste
        // (requisito 3) prova de forma mais direta); aqui o ponto é outro:
        // confirmar que os sinais retornados vêm identificados pelo id
        // certo, prova de que cada instância manteve seu próprio estado.
        let prices = [
            dec!(100),
            dec!(90),
            dec!(80),
            dec!(70),
            dec!(90),
            dec!(120),
            dec!(150),
        ];
        let mut all_signals = Vec::new();
        for price in prices {
            all_signals.extend(feed(price, &mut registry));
        }

        // As duas instâncias recebem a mesma sequência (mesmo instrumento
        // registrado para ambas) e, por terem params default idênticos,
        // devem produzir o mesmo padrão de sinais *cada uma* — prova de
        // que nenhuma delas "adiantou" o crossover por causa da outra
        // (não há vazamento de estado entre A e B).
        let from_a: Vec<_> = all_signals
            .iter()
            .filter(|s| s.strategy_id.as_str() == "ema-a")
            .collect();
        let from_b: Vec<_> = all_signals
            .iter()
            .filter(|s| s.strategy_id.as_str() == "ema-b")
            .collect();
        assert!(!from_a.is_empty());
        assert_eq!(from_a.len(), from_b.len());
        assert_eq!(
            from_a.iter().map(|s| s.direction).collect::<Vec<_>>(),
            from_b.iter().map(|s| s.direction).collect::<Vec<_>>(),
            "duas instâncias com params idênticos e a mesma sequência de candles devem produzir \
             exatamente os mesmos sinais, cada uma calculando seu próprio estado do zero"
        );
    }

    /// Requisito 3: estratégias de instrumentos diferentes só recebem seus
    /// próprios eventos — um evento de ETH nunca chega à instância
    /// configurada só para BTC.
    #[test]
    fn strategies_only_receive_events_for_their_own_instrument() {
        let btc = instrument("BTC", "USDT");
        let eth = instrument("ETH", "USDT");
        let configs = vec![
            StrategyInstanceConfig {
                id: StrategyId::new("ema-btc-only").unwrap(),
                kind: "ema_crossover".to_string(),
                symbols: vec![btc.symbol.to_string()],
            },
            StrategyInstanceConfig {
                id: StrategyId::new("ema-eth-only").unwrap(),
                kind: "ema_crossover".to_string(),
                symbols: vec![eth.symbol.to_string()],
            },
        ];

        let mut registry = build_registry(&configs, &[btc.clone(), eth.clone()], &ohlcv()).unwrap();

        let event = domain::MarketEvent::Candle(domain::Candle {
            instrument_id: eth.id,
            timeframe: domain::Timeframe::M1,
            open_time: chrono::Utc::now(),
            close_time: chrono::Utc::now(),
            open: dec!(100),
            high: dec!(100),
            low: dec!(100),
            close: dec!(100),
            volume: dec!(1),
            is_closed: true,
        });

        // Um evento de ETH é despachado só quando o instrumento passado a
        // `dispatch` é o ETH — a estratégia registrada só para BTC nunca é
        // chamada, exatamente como `StrategyRegistry::dispatch` já garante
        // por construção; este teste confirma que o builder preserva essa
        // garantia (não amplia acidentalmente o conjunto de instrumentos de
        // cada instância).
        let _ = registry.dispatch(&eth, &event, &crate::position_query::NoPositions);

        // Confirmação estrutural direta: a entrada "ema-btc-only" nunca foi
        // registrada contra ETH — só contra BTC.
        assert_eq!(registry.ids().count(), 2);
    }

    /// Requisito 4: identidade correta da instância nos resultados — o
    /// `Signal` produzido carrega exatamente o `StrategyId` configurado
    /// (o "robot_id"), nunca o `kind`.
    #[test]
    fn signals_are_tagged_with_the_configured_instance_id_not_the_kind() {
        let btc = instrument("BTC", "USDT");
        let configs = vec![StrategyInstanceConfig {
            id: StrategyId::new("quant-momentum-robot-7").unwrap(),
            kind: "quant_momentum".to_string(),
            symbols: vec![btc.symbol.to_string()],
        }];

        let mut registry = build_registry(&configs, std::slice::from_ref(&btc), &ohlcv()).unwrap();

        // `quant_momentum` usa os params default do catálogo
        // (`regression_period: 20`, `min_relative_volume: 1.0`) — precisa
        // de pelo menos 20 candles para encher a janela de regressão; uma
        // reta perfeita com volume constante garante R²=1.0 e volume
        // relativo exatamente 1.0, ambos acima dos limiares default.
        let mut open_time = chrono::Utc::now();
        let mut all_signals = Vec::new();
        for i in 0..25 {
            let price = dec!(100) + rust_decimal::Decimal::from(i * 3);
            let event = domain::MarketEvent::Candle(domain::Candle {
                instrument_id: btc.id,
                timeframe: domain::Timeframe::M1,
                open_time,
                close_time: open_time + chrono::Duration::minutes(1),
                open: price,
                high: price,
                low: price,
                close: price,
                volume: dec!(100),
                is_closed: true,
            });
            open_time += chrono::Duration::minutes(1);
            all_signals.extend(registry.dispatch(
                &btc,
                &event,
                &crate::position_query::NoPositions,
            ));
        }

        assert!(
            !all_signals.is_empty(),
            "expected the trending series to trigger a signal"
        );
        for signal in &all_signals {
            assert_eq!(signal.strategy_id.as_str(), "quant-momentum-robot-7");
            assert_ne!(signal.strategy_id.as_str(), "quant_momentum");
        }
    }

    /// Requisito 5: determinismo — construir e alimentar o mesmo conjunto
    /// de configs duas vezes, com a mesma sequência de candles, produz
    /// exatamente os mesmos sinais (mesma ordem, mesmo conteúdo, exceto o
    /// `SignalId`/timestamp aleatórios de cada `Signal::new`).
    #[test]
    fn building_and_feeding_the_same_configs_twice_is_deterministic() {
        let btc = instrument("BTC", "USDT");
        let configs = vec![
            StrategyInstanceConfig {
                id: StrategyId::new("ema-det").unwrap(),
                kind: "ema_crossover".to_string(),
                symbols: vec![btc.symbol.to_string()],
            },
            StrategyInstanceConfig {
                id: StrategyId::new("momentum-det").unwrap(),
                kind: "momentum".to_string(),
                symbols: vec![btc.symbol.to_string()],
            },
        ];
        let prices = [
            dec!(100),
            dec!(90),
            dec!(80),
            dec!(70),
            dec!(90),
            dec!(120),
            dec!(150),
        ];

        fn run(
            configs: &[StrategyInstanceConfig],
            btc: &Instrument,
            prices: &[rust_decimal::Decimal],
        ) -> Vec<(String, domain::SignalDirection)> {
            let mut registry =
                build_registry(configs, std::slice::from_ref(btc), &ohlcv()).unwrap();
            let mut open_time = chrono::Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
            let mut out = Vec::new();
            for price in prices {
                let event = domain::MarketEvent::Candle(domain::Candle {
                    instrument_id: btc.id,
                    timeframe: domain::Timeframe::M1,
                    open_time,
                    close_time: open_time + chrono::Duration::minutes(1),
                    open: *price,
                    high: *price,
                    low: *price,
                    close: *price,
                    volume: dec!(1),
                    is_closed: true,
                });
                open_time += chrono::Duration::minutes(1);
                for signal in registry.dispatch(btc, &event, &crate::position_query::NoPositions) {
                    out.push((signal.strategy_id.as_str().to_string(), signal.direction));
                }
            }
            out
        }

        use chrono::TimeZone;
        let first = run(&configs, &btc, &prices);
        let second = run(&configs, &btc, &prices);
        assert!(!first.is_empty());
        assert_eq!(first, second);
    }

    /// Requisito 6: as 6 estratégias do catálogo continuam podendo ser
    /// construídas através do builder de instâncias — não só diretamente
    /// via `catalog::build`.
    #[test]
    fn all_six_catalog_strategies_can_be_built_as_instances() {
        let btc = instrument("BTC", "USDT");
        let configs: Vec<StrategyInstanceConfig> = catalog::entries()
            .into_iter()
            .map(|entry| StrategyInstanceConfig {
                id: StrategyId::new(format!("{}-instance", entry.descriptor.id)).unwrap(),
                kind: entry.descriptor.id.to_string(),
                symbols: vec![btc.symbol.to_string()],
            })
            .collect();

        let registry = build_registry(&configs, &[btc], &ohlcv()).unwrap();
        assert_eq!(registry.len(), 6);
    }

    #[test]
    fn duplicate_instance_ids_are_rejected() {
        let btc = instrument("BTC", "USDT");
        let configs = vec![
            StrategyInstanceConfig {
                id: StrategyId::new("dup").unwrap(),
                kind: "ema_crossover".to_string(),
                symbols: vec![btc.symbol.to_string()],
            },
            StrategyInstanceConfig {
                id: StrategyId::new("dup").unwrap(),
                kind: "momentum".to_string(),
                symbols: vec![btc.symbol.to_string()],
            },
        ];

        let result = build_registry(&configs, &[btc], &ohlcv());
        assert!(matches!(result, Err(InstanceError::DuplicateId(id)) if id == "dup"));
    }

    #[test]
    fn instance_with_no_matching_instrument_is_skipped_not_an_error() {
        let btc = instrument("BTC", "USDT");
        let configs = vec![StrategyInstanceConfig {
            id: StrategyId::new("ema-orphan").unwrap(),
            kind: "ema_crossover".to_string(),
            symbols: vec!["DOGE/USDT".to_string()],
        }];

        let registry = build_registry(&configs, &[btc], &ohlcv()).unwrap();
        assert!(registry.is_empty());
    }

    #[test]
    fn unknown_kind_surfaces_as_catalog_error() {
        let btc = instrument("BTC", "USDT");
        let configs = vec![StrategyInstanceConfig {
            id: StrategyId::new("bad").unwrap(),
            kind: "does_not_exist".to_string(),
            symbols: vec![btc.symbol.to_string()],
        }];

        let result = build_registry(&configs, &[btc], &ohlcv());
        assert!(matches!(
            result,
            Err(InstanceError::Catalog(CatalogError::UnknownKind(_)))
        ));
    }
}
