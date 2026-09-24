//! # Reversão à Média (Z-Score)
//!
//! **Racional:** quando o preço se desvia muito de sua média móvel recente (em
//! unidades de desvio padrão), ele tem, historicamente e em certos regimes,
//! tendência a reverter em direção a essa média. Esta estratégia opera contra os
//! extremos em vez de segui-los — premissa oposta à de `MomentumStrategy` e
//! `EmaCrossoverStrategy`.
//!
//! **Entradas:** candles OHLCV fechados de um instrumento.
//!
//! **Parâmetros:** `window` (lookback para média/desvio padrão), `entry_z`
//! (z-score absoluto exigido para emitir um sinal).
//!
//! **Mercados suportados:** `Crypto`, `Equity`.
//!
//! **Dados necessários:** `MarketDataKind::Ohlcv`.
//!
//! **Limitações:**
//! - Nenhum sinal até que `window` candles fechados tenham sido observados.
//! - Se a série subjacente estiver em tendência em vez de lateralizada, operar
//!   contra o movimento vai diretamente contra a tendência vigente e pode
//!   acumular prejuízos (este é o modo de falha clássico das estratégias de
//!   reversão à média).
//! - O z-score assume uma distribuição aproximadamente estacionária ao longo de
//!   `window`; ele não detecta mudanças de regime.
//!
//! **Quando não usar:** em instrumentos/timeframes que exibem tendências
//! persistentes, ou imediatamente em torno de quebras estruturais conhecidas
//! (listagens, grandes eventos noticiosos) em que a média recente não é uma
//! âncora significativa.

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
pub struct MeanReversionParams {
    pub window: usize,
    pub entry_z: f64,
}

impl Default for MeanReversionParams {
    fn default() -> Self {
        Self {
            window: 20,
            entry_z: 2.0,
        }
    }
}

pub struct MeanReversionStrategy {
    id: StrategyId,
    params: MeanReversionParams,
    requirements: StrategyRequirements,
    windows: HashMap<InstrumentId, RollingWindow>,
}

impl MeanReversionStrategy {
    pub fn new(id: StrategyId, params: MeanReversionParams) -> Self {
        assert!(params.window > 1, "window must be greater than 1");
        assert!(params.entry_z > 0.0, "entry_z must be positive");
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

impl Strategy for MeanReversionStrategy {
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
            .or_insert_with(|| RollingWindow::new(self.params.window));
        window.push(close);

        if !window.is_full() {
            return None;
        }
        let mean = window.mean()?;
        let stddev = window.stddev()?;
        if stddev.abs() < f64::EPSILON {
            return None;
        }
        let z = (close - mean) / stddev;
        if z.abs() < self.params.entry_z {
            return None;
        }

        // Preço muito *acima* da média (z positivo) => espera-se reversão para
        // baixo => Short. Preço muito *abaixo* da média (z negativo) => Long.
        let direction = if z > 0.0 {
            SignalDirection::Short
        } else {
            SignalDirection::Long
        };
        let confidence = (z.abs() / (self.params.entry_z * 2.0)).clamp(0.0, 1.0);

        Signal::new(
            self.id.clone(),
            instrument.id,
            direction,
            confidence,
            candle.close_time,
            None,
            None,
            json!({ "z_score": z, "mean": mean, "stddev": stddev }),
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
        let base = Asset::new("SOL").unwrap();
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
    fn emits_short_on_extreme_spike_above_mean() {
        let mut strategy = MeanReversionStrategy::new(
            StrategyId::new("mr-test").unwrap(),
            MeanReversionParams {
                window: 5,
                entry_z: 1.0,
            },
        );
        let instrument = instrument();
        let prices = [
            dec!(100),
            dec!(100),
            dec!(100),
            dec!(100),
            dec!(100),
            dec!(150),
        ];
        let mut last_signal = None;
        for (i, price) in prices.iter().enumerate() {
            let event = candle_event(&instrument, *price, i as i64);
            last_signal =
                strategy.on_event(&instrument, &event, &crate::position_query::NoPositions);
        }
        assert_eq!(last_signal.unwrap().direction, SignalDirection::Short);
    }

    #[test]
    fn no_signal_on_flat_series() {
        let mut strategy = MeanReversionStrategy::new(
            StrategyId::new("mr-test").unwrap(),
            MeanReversionParams::default(),
        );
        let instrument = instrument();
        for i in 0..30 {
            let event = candle_event(&instrument, dec!(100), i);
            assert_eq!(
                strategy.on_event(&instrument, &event, &crate::position_query::NoPositions),
                None
            );
        }
    }
}
