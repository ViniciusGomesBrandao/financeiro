//! O mecanismo pelo qual uma estratégia consome `features::FeatureSnapshot`
//! em vez de recalcular indicadores por conta própria.
//!
//! Duas peças, deliberadamente pequenas:
//!
//! - [`FeatureStrategy`] — o trait que uma estratégia baseada em features
//!   implementa. Só quatro métodos: identidade, requisitos de
//!   compatibilidade (igual a `Strategy`), a configuração de janelas que a
//!   estratégia precisa (`feature_config`), e o processamento do snapshot
//!   já calculado (`on_features`). Não há acesso a `MarketEvent`/`Candle`
//!   bruto neste trait — uma estratégia que só implementa `FeatureStrategy`
//!   *não consegue* recalcular um indicador, porque nunca vê o candle, só
//!   o snapshot já pronto.
//! - [`FeatureStrategyAdapter`] — adapta qualquer `FeatureStrategy` para o
//!   trait `Strategy` que `StrategyRegistry` já sabe despachar, mantendo um
//!   `features::FeatureEngine` por instrumento (a mesma convenção de estado
//!   por-instrumento que `strategies::generic::indicators` já usa
//!   internamente nas três estratégias baseline). É a única peça que toca
//!   `MarketEvent`/`Candle` — o "cano" entre candles brutos e snapshots,
//!   isolado num só lugar em vez de duplicado em cada estratégia nova.
//!
//! Sem look-ahead: o adapter alimenta o `FeatureEngine` um candle fechado
//! por vez, na ordem em que `StrategyRegistry::dispatch` os entrega — a
//! mesma garantia estrutural de `features::FeatureEngine::update` (ver o
//! doc do crate `features`), agora estendida a qualquer `FeatureStrategy`.

use std::collections::HashMap;

use domain::{Instrument, InstrumentId, MarketDataKind, MarketEvent, Signal, StrategyId};
use features::{FeatureConfig, FeatureEngine, FeatureSnapshot};

use crate::position_query::PositionQuery;
use crate::requirements::StrategyRequirements;
use crate::strategy::Strategy;

/// Uma estratégia que consome features já calculadas, nunca dados de
/// mercado brutos. Implementações não devem manter seu próprio estado de
/// indicador (EMA, janela rolante, ...) — esse estado já vive dentro do
/// `FeatureEngine` que o `FeatureStrategyAdapter` mantém; o único estado
/// interno legítimo aqui é o que a *lógica de decisão* precisa lembrar
/// entre chamadas (ex.: o valor de uma feature na chamada anterior, para
/// comparar expansão/contração — ver `VolatilityBreakoutStrategy`), nunca
/// um recálculo do próprio indicador.
pub trait FeatureStrategy: Send {
    fn id(&self) -> &StrategyId;

    /// Mesmo contrato de `Strategy::requirements` — deve incluir
    /// `MarketDataKind::Ohlcv` (é o único tipo de dado que
    /// `FeatureStrategyAdapter` sabe transformar em features hoje; ver o
    /// aviso de escopo OHLCV-vs-microestrutura no crate `features`).
    fn requirements(&self) -> &StrategyRequirements;

    /// As janelas/períodos que esta estratégia precisa do
    /// `FeatureEngine` — a declaração explícita pedida: cada estratégia diz
    /// o que usa, em vez de um `FeatureConfig` global compartilhado e
    /// implícito. Chamado uma vez por instrumento, na primeira vez que o
    /// adapter precisa criar um `FeatureEngine` para ele.
    fn feature_config(&self) -> FeatureConfig;

    /// Processa um `FeatureSnapshot` já calculado e opcionalmente emite um
    /// sinal. Chamado uma vez por candle fechado, só depois que o
    /// `FeatureEngine` interno já absorveu aquele candle.
    ///
    /// `positions` é a única fonte de verdade sobre posição real (ver
    /// `PositionQuery`) — usada para decidir se este candle deve avaliar uma
    /// nova entrada ou monitorar a saída de uma posição que de fato existe,
    /// nunca de uma suposição interna sobre sinais emitidos anteriormente.
    fn on_features(
        &mut self,
        instrument: &Instrument,
        snapshot: &FeatureSnapshot,
        positions: &dyn PositionQuery,
    ) -> Option<Signal>;
}

/// Adapta um [`FeatureStrategy`] para [`Strategy`], mantendo um
/// `FeatureEngine` por instrumento. Ver o doc do módulo para o porquê desse
/// desenho.
pub struct FeatureStrategyAdapter<S: FeatureStrategy> {
    inner: S,
    feature_config: FeatureConfig,
    engines: HashMap<InstrumentId, FeatureEngine>,
}

impl<S: FeatureStrategy> FeatureStrategyAdapter<S> {
    pub fn new(inner: S) -> Self {
        debug_assert!(
            inner.requirements().requires(MarketDataKind::Ohlcv),
            "a FeatureStrategy must require MarketDataKind::Ohlcv — it is the only market data \
             kind FeatureStrategyAdapter knows how to turn into features"
        );
        let feature_config = inner.feature_config();
        Self {
            inner,
            feature_config,
            engines: HashMap::new(),
        }
    }
}

impl<S: FeatureStrategy> Strategy for FeatureStrategyAdapter<S> {
    fn id(&self) -> &StrategyId {
        self.inner.id()
    }

    fn requirements(&self) -> &StrategyRequirements {
        self.inner.requirements()
    }

    fn on_event(
        &mut self,
        instrument: &Instrument,
        event: &MarketEvent,
        positions: &dyn PositionQuery,
    ) -> Option<Signal> {
        let MarketEvent::Candle(candle) = event else {
            return None;
        };
        if !candle.is_closed {
            return None;
        }

        let engine = self
            .engines
            .entry(instrument.id)
            .or_insert_with(|| FeatureEngine::new(instrument.id, self.feature_config.clone()));
        let snapshot = engine.update(candle)?;

        self.inner.on_features(instrument, &snapshot, positions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use domain::{AssetClass, Exchange, MarketType, Symbol};
    use rust_decimal_macros::dec;
    use serde_json::json;

    struct RecordsSnapshots {
        id: StrategyId,
        requirements: StrategyRequirements,
        seen: Vec<FeatureSnapshot>,
    }

    impl FeatureStrategy for RecordsSnapshots {
        fn id(&self) -> &StrategyId {
            &self.id
        }
        fn requirements(&self) -> &StrategyRequirements {
            &self.requirements
        }
        fn feature_config(&self) -> FeatureConfig {
            FeatureConfig {
                sma_period: 3,
                ..FeatureConfig::default()
            }
        }
        fn on_features(
            &mut self,
            _instrument: &Instrument,
            snapshot: &FeatureSnapshot,
            _positions: &dyn PositionQuery,
        ) -> Option<Signal> {
            self.seen.push(snapshot.clone());
            // Emite um sinal Long sempre que a SMA já estiver disponível,
            // só para provar que o pipeline entrega o snapshot certo.
            snapshot.sma.map(|_| {
                Signal::new(
                    self.id.clone(),
                    _instrument.id,
                    domain::SignalDirection::Long,
                    0.5,
                    snapshot.timestamp,
                    None,
                    None,
                    json!({}),
                )
                .unwrap()
            })
        }
    }

    fn instrument() -> Instrument {
        let base = domain::Asset::new("BTC").unwrap();
        let quote = domain::Asset::new("USDT").unwrap();
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

    fn candle_event(
        instrument: &Instrument,
        close: rust_decimal::Decimal,
        minute: i64,
    ) -> MarketEvent {
        let open_time =
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap() + chrono::Duration::minutes(minute);
        MarketEvent::Candle(domain::Candle {
            instrument_id: instrument.id,
            timeframe: domain::Timeframe::M1,
            open_time,
            close_time: open_time + chrono::Duration::minutes(1),
            open: close,
            high: close,
            low: close,
            close,
            volume: dec!(1),
            is_closed: true,
        })
    }

    #[test]
    fn adapter_feeds_engine_and_forwards_snapshot_to_inner_strategy() {
        let inner = RecordsSnapshots {
            id: StrategyId::new("records").unwrap(),
            requirements: StrategyRequirements::new(
                vec![MarketDataKind::Ohlcv],
                vec![AssetClass::Crypto],
            ),
            seen: Vec::new(),
        };
        let mut adapter = FeatureStrategyAdapter::new(inner);
        let instrument = instrument();

        // sma_period=3: nenhum sinal até o 3º candle.
        assert_eq!(
            adapter.on_event(
                &instrument,
                &candle_event(&instrument, dec!(100), 0),
                &crate::position_query::NoPositions
            ),
            None
        );
        assert_eq!(
            adapter.on_event(
                &instrument,
                &candle_event(&instrument, dec!(101), 1),
                &crate::position_query::NoPositions
            ),
            None
        );
        let signal = adapter.on_event(
            &instrument,
            &candle_event(&instrument, dec!(102), 2),
            &crate::position_query::NoPositions,
        );
        assert!(signal.is_some());
    }

    #[test]
    fn ignores_non_candle_and_unclosed_candle_events() {
        let inner = RecordsSnapshots {
            id: StrategyId::new("records").unwrap(),
            requirements: StrategyRequirements::new(
                vec![MarketDataKind::Ohlcv],
                vec![AssetClass::Crypto],
            ),
            seen: Vec::new(),
        };
        let mut adapter = FeatureStrategyAdapter::new(inner);
        let instrument = instrument();

        let mut unclosed = candle_event(&instrument, dec!(100), 0);
        if let MarketEvent::Candle(c) = &mut unclosed {
            c.is_closed = false;
        }
        assert_eq!(
            adapter.on_event(&instrument, &unclosed, &crate::position_query::NoPositions),
            None
        );
    }
}
