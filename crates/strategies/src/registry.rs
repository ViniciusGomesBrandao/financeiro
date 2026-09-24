use std::collections::HashSet;

use domain::{Instrument, InstrumentId, MarketDataKind, MarketEvent, Signal};
use tracing::info;

use crate::error::CompatibilityError;
use crate::position_query::{ForStrategy, RobotPositions};
use crate::strategy::Strategy;

fn event_kind(event: &MarketEvent) -> MarketDataKind {
    match event {
        MarketEvent::Candle(_) => MarketDataKind::Ohlcv,
        MarketEvent::Trade(_) => MarketDataKind::Trades,
        MarketEvent::OrderBook(_) => MarketDataKind::OrderBookL1,
    }
}

/// Verifica se `strategy` pode rodar contra `instrument`, dado o conjunto de
/// tipos de dados de mercado efetivamente disponíveis para ele (a partir do(s)
/// `MarketDataProvider`(s) configurado(s)). Este é o único ponto de
/// estrangulamento que impede uma estratégia de rodar silenciosamente em um
/// mercado incompatível — chame-o imediatamente no momento do
/// registro/configuração, nunca preguiçosamente no momento do sinal.
pub fn check_compatible(
    strategy: &dyn Strategy,
    instrument: &Instrument,
    available_market_data: &HashSet<MarketDataKind>,
) -> Result<(), CompatibilityError> {
    let requirements = strategy.requirements();

    if !requirements.supports(instrument.asset_class) {
        return Err(CompatibilityError::UnsupportedAssetClass {
            strategy_id: strategy.id().to_string(),
            symbol: instrument.symbol.to_string(),
            asset_class: instrument.asset_class,
        });
    }

    for required in &requirements.required_market_data {
        if !available_market_data.contains(required) {
            return Err(CompatibilityError::MissingMarketData {
                strategy_id: strategy.id().to_string(),
                symbol: instrument.symbol.to_string(),
                missing: *required,
            });
        }
    }

    Ok(())
}

struct RegisteredStrategy {
    strategy: Box<dyn Strategy>,
    instruments: HashSet<InstrumentId>,
}

/// Guarda as estratégias configuradas para esta execução e os instrumentos que
/// cada uma está aprovada a negociar. O registro é a *única* forma de uma
/// estratégia se tornar elegível a receber eventos — não existe caminho que
/// contorne `check_compatible`.
#[derive(Default)]
pub struct StrategyRegistry {
    entries: Vec<RegisteredStrategy>,
}

impl StrategyRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra `strategy` contra `instruments`, rejeitando a chamada inteira se
    /// *qualquer* instrumento for incompatível (sem registro parcial).
    pub fn register(
        &mut self,
        strategy: Box<dyn Strategy>,
        instruments: &[Instrument],
        available_market_data: &HashSet<MarketDataKind>,
    ) -> Result<(), CompatibilityError> {
        for instrument in instruments {
            check_compatible(strategy.as_ref(), instrument, available_market_data)?;
        }

        info!(
            strategy_id = %strategy.id(),
            instruments = instruments.len(),
            "strategy registered"
        );

        self.entries.push(RegisteredStrategy {
            strategy,
            instruments: instruments.iter().map(|i| i.id).collect(),
        });
        Ok(())
    }

    /// Despacha `event` (referente a `instrument`) para toda estratégia
    /// registrada que (a) está aprovada a negociar aquele instrumento e (b)
    /// declarou um requisito correspondente ao tipo de dado deste evento.
    /// Retorna todos os sinais produzidos, na ordem de registro.
    ///
    /// `positions` é a fonte multi-robô de posições reais (ver
    /// `RobotPositions`, ADR-20) — para cada estratégia despachada, o
    /// registry constrói um `ForStrategy` escopado ao `StrategyId` dela
    /// antes de chamar `Strategy::on_event`, para que nenhuma estratégia
    /// possa enxergar a posição de outro robô no mesmo instrumento.
    pub fn dispatch(
        &mut self,
        instrument: &Instrument,
        event: &MarketEvent,
        positions: &dyn RobotPositions,
    ) -> Vec<Signal> {
        let kind = event_kind(event);
        let mut signals = Vec::new();

        for entry in &mut self.entries {
            if !entry.instruments.contains(&instrument.id) {
                continue;
            }
            if !entry.strategy.requirements().requires(kind) {
                continue;
            }
            // Clonado (não emprestado de `entry.strategy`) de propósito:
            // `on_event` logo abaixo precisa de `&mut entry.strategy`, o
            // que não coexistiria com um empréstimo imutável vivo de
            // `entry.strategy.id()`.
            let strategy_id = entry.strategy.id().clone();
            let scoped = ForStrategy {
                source: positions,
                strategy_id: &strategy_id,
            };
            if let Some(signal) = entry.strategy.on_event(instrument, event, &scoped) {
                info!(
                    strategy_id = %entry.strategy.id(),
                    symbol = %instrument.symbol,
                    direction = ?signal.direction,
                    confidence = signal.confidence,
                    "strategy signal"
                );
                signals.push(signal);
            }
        }

        signals
    }

    /// Como [`dispatch`], mas só chama estratégias cujo id está em
    /// `only_ids`. Usado quando vários robôs compartilham o mesmo
    /// instrumento em timeframes diferentes — cada candle só deve aquecer
    /// / sinalizar as instâncias do robô daquele timeframe.
    pub fn dispatch_ids(
        &mut self,
        instrument: &Instrument,
        event: &MarketEvent,
        positions: &dyn RobotPositions,
        only_ids: &[domain::StrategyId],
    ) -> Vec<Signal> {
        if only_ids.is_empty() {
            return Vec::new();
        }
        let kind = event_kind(event);
        let mut signals = Vec::new();

        for entry in &mut self.entries {
            if !entry.instruments.contains(&instrument.id) {
                continue;
            }
            let strategy_id = entry.strategy.id().clone();
            if !only_ids.iter().any(|id| id == &strategy_id) {
                continue;
            }
            if !entry.strategy.requirements().requires(kind) {
                continue;
            }
            let scoped = ForStrategy {
                source: positions,
                strategy_id: &strategy_id,
            };
            if let Some(signal) = entry.strategy.on_event(instrument, event, &scoped) {
                info!(
                    strategy_id = %entry.strategy.id(),
                    symbol = %instrument.symbol,
                    direction = ?signal.direction,
                    confidence = signal.confidence,
                    "strategy signal"
                );
                signals.push(signal);
            }
        }

        signals
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Busca a estratégia registrada com este id, para introspecção
    /// (ex.: um dashboard futuro listando o que está rodando agora nesta
    /// execução) — leitura pura, não afeta `dispatch`. Não confundir com
    /// `catalog::get`: aquele resolve "quais estratégias *existem*"
    /// (o catálogo estático); este resolve "o que está *registrado* nesta
    /// execução agora" (o estado do dispatcher).
    pub fn get(&self, id: &domain::StrategyId) -> Option<&dyn Strategy> {
        self.entries
            .iter()
            .find(|entry| entry.strategy.id() == id)
            .map(|entry| entry.strategy.as_ref())
    }

    /// Ids de todas as estratégias registradas, na ordem de registro —
    /// pode conter o mesmo id mais de uma vez se ele foi registrado para
    /// grupos de instrumentos diferentes em chamadas separadas (`register`
    /// não deduplica por id de propósito: nada nesta camada impede duas
    /// instâncias independentes com o mesmo `StrategyId` lógico).
    pub fn ids(&self) -> impl Iterator<Item = &domain::StrategyId> {
        self.entries.iter().map(|entry| entry.strategy.id())
    }

    /// Ids de todas as estratégias registradas contra `instrument_id`
    /// especificamente — os candidatos que um seletor (ex.: um futuro
    /// Strategy Judge) deve considerar para este instrumento, sem
    /// duplicar aqui a associação estratégia↔instrumento que o registry
    /// já mantém internamente.
    pub fn strategy_ids_for_instrument(
        &self,
        instrument_id: domain::InstrumentId,
    ) -> Vec<domain::StrategyId> {
        self.entries
            .iter()
            .filter(|entry| entry.instruments.contains(&instrument_id))
            .map(|entry| entry.strategy.id().clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generic::{EmaCrossoverParams, EmaCrossoverStrategy};
    use crate::position_query::PositionQuery;
    use crate::requirements::StrategyRequirements;
    use chrono::Utc;
    use domain::{Asset, AssetClass, Exchange, MarketType, Symbol};
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn crypto_instrument() -> Instrument {
        let base = Asset::new("BTC").unwrap();
        let quote = Asset::new("USDT").unwrap();
        Instrument::new(
            Symbol::from_pair(&base, &quote),
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

    fn equity_instrument() -> Instrument {
        let base = Asset::new("AAPL").unwrap();
        let quote = Asset::new("USD").unwrap();
        Instrument::new(
            Symbol::from_pair(&base, &quote),
            base,
            quote,
            AssetClass::Equity,
            Exchange::Other("Nasdaq".to_string()),
            MarketType::Equity,
            dec!(0.01),
            dec!(1),
            dec!(1),
            dec!(1),
        )
    }

    struct FundingOnlyStrategy {
        id: domain::StrategyId,
        requirements: StrategyRequirements,
    }

    impl Strategy for FundingOnlyStrategy {
        fn id(&self) -> &domain::StrategyId {
            &self.id
        }
        fn requirements(&self) -> &StrategyRequirements {
            &self.requirements
        }
        fn on_event(
            &mut self,
            _instrument: &Instrument,
            _event: &MarketEvent,
            _positions: &dyn PositionQuery,
        ) -> Option<Signal> {
            None
        }
    }

    #[test]
    fn rejects_unsupported_asset_class() {
        let strategy = EmaCrossoverStrategy::new(
            domain::StrategyId::new("ema").unwrap(),
            EmaCrossoverParams::default(),
        );
        // O cruzamento de EMAs suporta Crypto/Equity, mas não CryptoDerivative.
        let mut instrument = crypto_instrument();
        instrument.asset_class = AssetClass::CryptoDerivative;
        let available = HashSet::from([MarketDataKind::Ohlcv]);

        let err = check_compatible(&strategy, &instrument, &available).unwrap_err();
        assert!(matches!(
            err,
            CompatibilityError::UnsupportedAssetClass { .. }
        ));
    }

    #[test]
    fn rejects_missing_market_data() {
        let strategy = FundingOnlyStrategy {
            id: domain::StrategyId::new("funding-only").unwrap(),
            requirements: StrategyRequirements::new(
                vec![MarketDataKind::FundingRate],
                vec![AssetClass::CryptoDerivative],
            ),
        };
        let mut instrument = crypto_instrument();
        instrument.asset_class = AssetClass::CryptoDerivative;
        let available = HashSet::from([MarketDataKind::Ohlcv]); // sem funding rate disponível

        let err = check_compatible(&strategy, &instrument, &available).unwrap_err();
        assert!(matches!(err, CompatibilityError::MissingMarketData { .. }));
    }

    #[test]
    fn accepts_compatible_instrument() {
        let strategy = EmaCrossoverStrategy::new(
            domain::StrategyId::new("ema").unwrap(),
            EmaCrossoverParams::default(),
        );
        let instrument = crypto_instrument();
        let available = HashSet::from([MarketDataKind::Ohlcv]);
        assert!(check_compatible(&strategy, &instrument, &available).is_ok());
    }

    #[test]
    fn registry_refuses_to_register_incompatible_strategy() {
        let mut registry = StrategyRegistry::new();
        let strategy = EmaCrossoverStrategy::new(
            domain::StrategyId::new("ema").unwrap(),
            EmaCrossoverParams::default(),
        );
        let equity = equity_instrument();
        let available = HashSet::from([MarketDataKind::Trades]); // Ohlcv indisponível neste feed

        let result = registry.register(Box::new(strategy), &[equity], &available);
        assert!(result.is_err());
        assert!(registry.is_empty());
    }

    #[test]
    fn registry_dispatches_only_to_registered_instruments() {
        let mut registry = StrategyRegistry::new();
        let strategy = EmaCrossoverStrategy::new(
            domain::StrategyId::new("ema").unwrap(),
            EmaCrossoverParams::default(),
        );
        let registered = crypto_instrument();
        let unregistered = crypto_instrument();
        let available = HashSet::from([MarketDataKind::Ohlcv]);

        registry
            .register(
                Box::new(strategy),
                std::slice::from_ref(&registered),
                &available,
            )
            .unwrap();

        let event = MarketEvent::Candle(domain::Candle {
            instrument_id: unregistered.id,
            timeframe: domain::Timeframe::M1,
            open_time: Utc::now(),
            close_time: Utc::now(),
            open: dec!(100),
            high: dec!(100),
            low: dec!(100),
            close: dec!(100),
            volume: dec!(1),
            is_closed: true,
        });

        // O evento é de um instrumento que nunca foi registrado contra esta
        // estratégia, então o dispatch não deve chamá-la.
        let signals = registry.dispatch(&unregistered, &event, &crate::position_query::NoPositions);
        assert!(signals.is_empty());
    }

    /// Prova que uma `FeatureStrategy` roda pelo mesmo `StrategyRegistry`
    /// que as baselines, sem nenhum caminho especial: registra
    /// `QuantMomentumStrategy` envolvida em `FeatureStrategyAdapter`
    /// (exatamente como uma baseline seria registrada, só trocando o
    /// `Box<dyn Strategy>` construído), despacha uma sequência de candles
    /// em tendência limpa e confirma que um sinal sai do outro lado.
    #[test]
    fn feature_strategy_dispatches_through_the_same_registry_as_baselines() {
        use crate::feature_strategy::FeatureStrategyAdapter;
        use crate::generic::{QuantMomentumParams, QuantMomentumStrategy};

        let mut registry = StrategyRegistry::new();
        let strategy = QuantMomentumStrategy::new(
            domain::StrategyId::new("qm-registry-test").unwrap(),
            QuantMomentumParams {
                regression_period: 5,
                min_r_squared: 0.9,
                relative_volume_period: 5,
                min_relative_volume: 0.0,
                ..QuantMomentumParams::default()
            },
        );
        let instrument = crypto_instrument();
        let available = HashSet::from([MarketDataKind::Ohlcv]);

        registry
            .register(
                Box::new(FeatureStrategyAdapter::new(strategy)),
                std::slice::from_ref(&instrument),
                &available,
            )
            .unwrap();

        let prices = [
            dec!(100),
            dec!(102),
            dec!(104),
            dec!(106),
            dec!(108),
            dec!(110),
        ];
        // `QuantMomentumStrategy` agora tem saída própria e explícita (ver
        // o audit semântico das três estratégias novas): uma vez com uma
        // posição real aberta, ela para de reavaliar entradas para este
        // instrumento. Como a estratégia não guarda mais esse estado
        // sozinha (ver `crate::position_query`), este teste simula o
        // Portfolio real sendo sincronizado após cada entrada aprovada —
        // exatamente o que `backtest`/`app` fazem passando o
        // `PortfolioManager` de verdade — para que o sinal Long ainda
        // apareça só uma vez, na primeira vez que a janela de regressão
        // enche. Por isso coleta os sinais de *todos* os candles, em vez de
        // assumir que só o último dispatch importa.
        let mut positions: HashMap<InstrumentId, domain::Side> = HashMap::new();
        let mut all_signals = Vec::new();
        for (i, price) in prices.iter().enumerate() {
            let open_time = Utc::now() + chrono::Duration::minutes(i as i64);
            let event = MarketEvent::Candle(domain::Candle {
                instrument_id: instrument.id,
                timeframe: domain::Timeframe::M1,
                open_time,
                close_time: open_time + chrono::Duration::minutes(1),
                open: *price,
                high: *price,
                low: *price,
                close: *price,
                volume: dec!(10),
                is_closed: true,
            });
            let signals = registry.dispatch(&instrument, &event, &positions);
            for signal in &signals {
                match signal.direction {
                    domain::SignalDirection::Long => {
                        positions.insert(instrument.id, domain::Side::Buy);
                    }
                    domain::SignalDirection::Short => {
                        positions.insert(instrument.id, domain::Side::Sell);
                    }
                    domain::SignalDirection::Flat => {
                        positions.remove(&instrument.id);
                    }
                }
            }
            all_signals.extend(signals);
        }

        assert_eq!(all_signals.len(), 1);
        assert_eq!(all_signals[0].direction, domain::SignalDirection::Long);
    }

    #[test]
    fn get_and_ids_expose_what_is_currently_registered() {
        let mut registry = StrategyRegistry::new();
        let id = domain::StrategyId::new("ema-introspection").unwrap();
        let strategy = EmaCrossoverStrategy::new(id.clone(), EmaCrossoverParams::default());
        let instrument = crypto_instrument();
        let available = HashSet::from([MarketDataKind::Ohlcv]);

        assert!(registry.get(&id).is_none());
        assert_eq!(registry.ids().count(), 0);

        registry
            .register(
                Box::new(strategy),
                std::slice::from_ref(&instrument),
                &available,
            )
            .unwrap();

        assert_eq!(registry.get(&id).unwrap().id(), &id);
        assert_eq!(registry.ids().collect::<Vec<_>>(), vec![&id]);

        let unknown = domain::StrategyId::new("does-not-exist").unwrap();
        assert!(registry.get(&unknown).is_none());
    }

    #[test]
    fn strategy_ids_for_instrument_only_returns_strategies_registered_for_it() {
        let mut registry = StrategyRegistry::new();
        let id_a = domain::StrategyId::new("ema-a").unwrap();
        let id_b = domain::StrategyId::new("ema-b").unwrap();
        let btc = crypto_instrument();
        let other_btc = crypto_instrument(); // instrumento diferente, mesma asset class
        let available = HashSet::from([MarketDataKind::Ohlcv]);

        registry
            .register(
                Box::new(EmaCrossoverStrategy::new(
                    id_a.clone(),
                    EmaCrossoverParams::default(),
                )),
                std::slice::from_ref(&btc),
                &available,
            )
            .unwrap();
        registry
            .register(
                Box::new(EmaCrossoverStrategy::new(
                    id_b.clone(),
                    EmaCrossoverParams::default(),
                )),
                std::slice::from_ref(&other_btc),
                &available,
            )
            .unwrap();

        assert_eq!(registry.strategy_ids_for_instrument(btc.id), vec![id_a]);
        assert_eq!(
            registry.strategy_ids_for_instrument(other_btc.id),
            vec![id_b]
        );
        assert!(registry
            .strategy_ids_for_instrument(domain::InstrumentId::new())
            .is_empty());
    }

    #[test]
    fn dispatch_ids_skips_strategies_not_in_the_allow_list() {
        let mut registry = StrategyRegistry::new();
        let id_a = domain::StrategyId::new("robot-a::ema").unwrap();
        let id_b = domain::StrategyId::new("robot-b::ema").unwrap();
        let instrument = crypto_instrument();
        let available = HashSet::from([MarketDataKind::Ohlcv]);

        registry
            .register(
                Box::new(EmaCrossoverStrategy::new(
                    id_a.clone(),
                    EmaCrossoverParams {
                        fast_period: 2,
                        slow_period: 4,
                    },
                )),
                std::slice::from_ref(&instrument),
                &available,
            )
            .unwrap();
        registry
            .register(
                Box::new(EmaCrossoverStrategy::new(
                    id_b.clone(),
                    EmaCrossoverParams {
                        fast_period: 2,
                        slow_period: 4,
                    },
                )),
                std::slice::from_ref(&instrument),
                &available,
            )
            .unwrap();

        let prices = [
            dec!(100),
            dec!(90),
            dec!(80),
            dec!(70),
            dec!(90),
            dec!(120),
            dec!(150),
        ];
        let mut signals = Vec::new();
        for (i, price) in prices.into_iter().enumerate() {
            let open_time = Utc::now() + chrono::Duration::minutes(i as i64);
            let candle = domain::Candle {
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
            };
            signals.extend(registry.dispatch_ids(
                &instrument,
                &MarketEvent::Candle(candle),
                &crate::position_query::NoPositions,
                &[id_a.clone()],
            ));
        }

        assert!(
            !signals.is_empty(),
            "expected crossover signal for robot-a after synthetic series"
        );
        assert!(
            signals.iter().all(|s| s.strategy_id == id_a),
            "robot-b must not receive events when filtered out of dispatch_ids"
        );
        assert!(!signals.iter().any(|s| s.strategy_id == id_b));
    }
}
