//! # Cruzamento de EMAs
//!
//! **Racional:** uma EMA rápida cruzando acima de uma EMA lenta é uma heurística
//! clássica de seguimento de tendência — reage a uma mudança no preço médio de
//! curto prazo em relação à tendência de mais longo prazo. Ela não diz nada
//! sobre *por que* o preço se moveu e não tem garantia de edge; existe aqui
//! principalmente para exercitar o pipeline ingestão → estratégia → sinal →
//! risco → paper trading de ponta a ponta.
//!
//! **Entradas:** candles OHLCV fechados de um instrumento.
//!
//! **Parâmetros:** `fast_period`, `slow_period` (comprimentos das EMAs, em
//! candles).
//!
//! **Mercados suportados:** `Crypto`, `Equity` — o cálculo subjacente (uma EMA
//! de preços de fechamento) faz sentido para qualquer instrumento com barras
//! OHLCV regulares.
//!
//! **Dados necessários:** `MarketDataKind::Ohlcv`.
//!
//! **Limitações:**
//! - Atrasada por construção: confirma uma tendência depois que ela já começou,
//!   por isso tem desempenho pior em mercados agitados/lateralizados
//!   (cruzamentos falsos frequentes, "whipsaws").
//! - Emite seu primeiro sinal significativo apenas depois que ambas as EMAs
//!   receberam ao menos uma atualização anterior — o primeiro candle nunca pode
//!   ser, por si só, um cruzamento.
//! - A confiança é uma heurística derivada da distância relativa entre as duas
//!   EMAs, não uma probabilidade calibrada.
//!
//! **Quando não usar:** instrumentos de baixa liquidez com formação errática de
//! candles, ou mercados conhecidos por lateralizar/reverter à média no timeframe
//! escolhido (nesse caso, considere `MeanReversionStrategy`).

use std::collections::HashMap;

use domain::{
    AssetClass, Instrument, InstrumentId, MarketDataKind, MarketEvent, Signal, SignalDirection,
    StrategyId,
};
use rust_decimal::prelude::ToPrimitive;
use serde::Serialize;
use serde_json::json;

use crate::generic::indicators::Ema;
use crate::position_query::PositionQuery;
use crate::requirements::StrategyRequirements;
use crate::strategy::Strategy;

/// `Serialize` existe só para `catalog::StrategyDescriptor::default_params`
/// (descoberta pelo dashboard) — nunca usado para desserializar/injetar
/// parâmetros de volta (não há mecanismo de override nesta fase).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct EmaCrossoverParams {
    pub fast_period: usize,
    pub slow_period: usize,
}

impl Default for EmaCrossoverParams {
    fn default() -> Self {
        Self {
            fast_period: 12,
            slow_period: 26,
        }
    }
}

struct InstrumentState {
    fast: Ema,
    slow: Ema,
    prev_diff: Option<f64>,
}

pub struct EmaCrossoverStrategy {
    id: StrategyId,
    params: EmaCrossoverParams,
    requirements: StrategyRequirements,
    state: HashMap<InstrumentId, InstrumentState>,
}

impl EmaCrossoverStrategy {
    pub fn new(id: StrategyId, params: EmaCrossoverParams) -> Self {
        assert!(
            params.fast_period < params.slow_period,
            "fast_period must be shorter than slow_period"
        );
        Self {
            id,
            params,
            requirements: StrategyRequirements::new(
                vec![MarketDataKind::Ohlcv],
                vec![AssetClass::Crypto, AssetClass::Equity],
            ),
            state: HashMap::new(),
        }
    }
}

impl Strategy for EmaCrossoverStrategy {
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

        let state = self
            .state
            .entry(instrument.id)
            .or_insert_with(|| InstrumentState {
                fast: Ema::new(self.params.fast_period),
                slow: Ema::new(self.params.slow_period),
                prev_diff: None,
            });

        let fast = state.fast.update(close);
        let slow = state.slow.update(close);
        let diff = fast - slow;
        let prev_diff = state.prev_diff.replace(diff);

        let prev_diff = prev_diff?;
        let direction = if prev_diff <= 0.0 && diff > 0.0 {
            SignalDirection::Long
        } else if prev_diff >= 0.0 && diff < 0.0 {
            SignalDirection::Short
        } else {
            return None;
        };

        let confidence = if slow.abs() > f64::EPSILON {
            (diff.abs() / slow.abs()).clamp(0.0, 1.0)
        } else {
            0.0
        };

        Signal::new(
            self.id.clone(),
            instrument.id,
            direction,
            confidence,
            candle.close_time,
            None,
            None,
            json!({ "fast_ema": fast, "slow_ema": slow }),
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
    fn requires_fast_shorter_than_slow() {
        let result = std::panic::catch_unwind(|| {
            EmaCrossoverStrategy::new(
                StrategyId::new("bad").unwrap(),
                EmaCrossoverParams {
                    fast_period: 26,
                    slow_period: 12,
                },
            )
        });
        assert!(result.is_err());
    }

    #[test]
    fn detects_bullish_crossover() {
        let mut strategy = EmaCrossoverStrategy::new(
            StrategyId::new("ema-test").unwrap(),
            EmaCrossoverParams {
                fast_period: 2,
                slow_period: 4,
            },
        );
        let instrument = instrument();

        // Alimenta uma série de queda seguida de alta, para que a EMA rápida
        // caia abaixo da lenta e depois volte a subir acima dela.
        let prices: Vec<Decimal> = vec![
            dec!(100),
            dec!(90),
            dec!(80),
            dec!(70),
            dec!(90),
            dec!(120),
            dec!(150),
        ];

        let mut signals = Vec::new();
        for (i, price) in prices.iter().enumerate() {
            let event = candle_event(&instrument, *price, i as i64);
            if let Some(signal) =
                strategy.on_event(&instrument, &event, &crate::position_query::NoPositions)
            {
                signals.push(signal);
            }
        }

        assert!(
            signals.iter().any(|s| s.direction == SignalDirection::Long),
            "expected at least one bullish crossover signal, got {signals:?}"
        );
    }

    #[test]
    fn ignores_unclosed_candles() {
        let mut strategy = EmaCrossoverStrategy::new(
            StrategyId::new("ema-test").unwrap(),
            EmaCrossoverParams::default(),
        );
        let instrument = instrument();
        let mut event = candle_event(&instrument, dec!(100), 0);
        if let MarketEvent::Candle(c) = &mut event {
            c.is_closed = false;
        }
        assert_eq!(
            strategy.on_event(&instrument, &event, &crate::position_query::NoPositions),
            None
        );
    }
}
