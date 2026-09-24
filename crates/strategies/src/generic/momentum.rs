//! # Momentum Simples
//!
//! **Racional:** um preço que se moveu fortemente em uma direção ao longo dos
//! últimos `lookback` candles tende, em média e em certos regimes, a continuar se
//! movendo naquela direção por mais algum tempo ("momentum" / continuação de
//! tendência). Esta é uma medida simplista de taxa de variação, não um escore de
//! momentum ajustado ao risco.
//!
//! **Entradas:** candles OHLCV fechados de um instrumento.
//!
//! **Parâmetros:** `lookback` (número de candles), `threshold` (retorno absoluto
//! mínimo, como fração, exigido para emitir um sinal — filtra movimentos no
//! nível do ruído).
//!
//! **Mercados suportados:** `Crypto`, `Equity`.
//!
//! **Dados necessários:** `MarketDataKind::Ohlcv`.
//!
//! **Limitações:**
//! - Nenhum sinal é produzido até que `lookback` candles fechados tenham sido
//!   observados para aquele instrumento.
//! - O momentum reverte abruptamente em pontos de inflexão; esta estratégia não
//!   tem mecanismo para detectar uma reversão antecipadamente.
//! - `threshold` é um filtro de ruído arbitrário, não um nível de significância
//!   derivado estatisticamente.
//!
//! **Quando não usar:** em instrumentos com reversões bruscas frequentes (esta
//! estratégia vai "correr atrás" do movimento e tipicamente ficar atrasada), ou
//! com janelas de lookback menores que o horizonte típico de ruído do
//! instrumento.

use std::collections::HashMap;

use domain::{
    AssetClass, Instrument, InstrumentId, MarketDataKind, MarketEvent, Signal, SignalDirection,
    StrategyId,
};
use rust_decimal::prelude::ToPrimitive;
use serde::Serialize;
use serde_json::json;

use crate::generic::indicators::RollingWindow;
use crate::position_query::PositionQuery;
use crate::requirements::StrategyRequirements;
use crate::strategy::Strategy;

/// `Serialize` existe só para `catalog::StrategyDescriptor::default_params`
/// (descoberta pelo dashboard) — nunca usado para desserializar/injetar
/// parâmetros de volta (não há mecanismo de override nesta fase).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct MomentumParams {
    pub lookback: usize,
    pub threshold: f64,
}

impl Default for MomentumParams {
    fn default() -> Self {
        Self {
            lookback: 10,
            threshold: 0.01,
        }
    }
}

pub struct MomentumStrategy {
    id: StrategyId,
    params: MomentumParams,
    requirements: StrategyRequirements,
    windows: HashMap<InstrumentId, RollingWindow>,
}

impl MomentumStrategy {
    pub fn new(id: StrategyId, params: MomentumParams) -> Self {
        assert!(params.lookback > 1, "lookback must be greater than 1");
        assert!(params.threshold >= 0.0, "threshold must be non-negative");
        Self {
            id,
            params,
            requirements: StrategyRequirements::new(
                vec![MarketDataKind::Ohlcv],
                vec![AssetClass::Crypto, AssetClass::Equity],
            ),
            windows: HashMap::new(),
        }
    }
}

impl Strategy for MomentumStrategy {
    fn id(&self) -> &StrategyId {
        &self.id
    }

    fn requirements(&self) -> &StrategyRequirements {
        &self.requirements
    }

    fn on_event(
        &mut self,
        instrument: &Instrument,
        event: &MarketEvent,
        _positions: &dyn PositionQuery,
    ) -> Option<Signal> {
        let candle = match event {
            MarketEvent::Candle(c) if c.is_closed => c,
            _ => return None,
        };
        let close = candle.close.to_f64()?;

        let window = self
            .windows
            .entry(instrument.id)
            .or_insert_with(|| RollingWindow::new(self.params.lookback));
        window.push(close);

        if !window.is_full() {
            return None;
        }
        let oldest = window.first()?;
        if oldest.abs() < f64::EPSILON {
            return None;
        }
        let ret = (close - oldest) / oldest;
        if ret.abs() < self.params.threshold {
            return None;
        }

        let direction = if ret > 0.0 {
            SignalDirection::Long
        } else {
            SignalDirection::Short
        };
        let confidence = (ret.abs() / (self.params.threshold * 5.0)).clamp(0.0, 1.0);

        Signal::new(
            self.id.clone(),
            instrument.id,
            direction,
            confidence,
            candle.close_time,
            None,
            None,
            json!({ "lookback_return": ret }),
        )
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use domain::{Asset, Exchange, MarketType, Symbol};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn instrument() -> Instrument {
        let base = Asset::new("ETH").unwrap();
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

    fn candle_event(instrument: &Instrument, close: Decimal, minute: i64) -> MarketEvent {
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
    fn no_signal_before_window_is_full() {
        let mut strategy = MomentumStrategy::new(
            StrategyId::new("mom-test").unwrap(),
            MomentumParams {
                lookback: 5,
                threshold: 0.01,
            },
        );
        let instrument = instrument();
        for i in 0..4 {
            let event = candle_event(&instrument, dec!(100), i);
            assert_eq!(
                strategy.on_event(&instrument, &event, &crate::position_query::NoPositions),
                None
            );
        }
    }

    #[test]
    fn emits_long_on_strong_upward_move() {
        let mut strategy = MomentumStrategy::new(
            StrategyId::new("mom-test").unwrap(),
            MomentumParams {
                lookback: 3,
                threshold: 0.01,
            },
        );
        let instrument = instrument();
        let prices = [dec!(100), dec!(101), dec!(103), dec!(120)];
        let mut last_signal = None;
        for (i, price) in prices.iter().enumerate() {
            let event = candle_event(&instrument, *price, i as i64);
            last_signal =
                strategy.on_event(&instrument, &event, &crate::position_query::NoPositions);
        }
        assert_eq!(last_signal.unwrap().direction, SignalDirection::Long);
    }

    #[test]
    fn suppresses_signal_below_threshold() {
        let mut strategy = MomentumStrategy::new(
            StrategyId::new("mom-test").unwrap(),
            MomentumParams {
                lookback: 3,
                threshold: 0.5,
            },
        );
        let instrument = instrument();
        let prices = [dec!(100), dec!(100.5), dec!(101), dec!(101.5)];
        for (i, price) in prices.iter().enumerate() {
            let event = candle_event(&instrument, *price, i as i64);
            assert_eq!(
                strategy.on_event(&instrument, &event, &crate::position_query::NoPositions),
                None
            );
        }
    }
}
