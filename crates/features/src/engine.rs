//! `FeatureEngine`: orquestra todos os calculadores de `ohlcv` para um
//! único instrumento, expondo uma única chamada por candle.

use domain::{Candle, InstrumentId};
use rust_decimal::prelude::ToPrimitive;

use crate::config::FeatureConfig;
use crate::ohlcv::atr::Atr;
use crate::ohlcv::autocorrelation::Autocorrelation;
use crate::ohlcv::bollinger::Bollinger;
use crate::ohlcv::momentum::Roc;
use crate::ohlcv::moving_average::{EmaFeature, Sma};
use crate::ohlcv::regression::RollingLinearRegression;
use crate::ohlcv::returns::MultiHorizonReturns;
use crate::ohlcv::rsi::Rsi;
use crate::ohlcv::volatility::{EwmaVolatility, PriceStdDev, RealizedVolatility};
use crate::ohlcv::volume::RelativeVolume;
use crate::ohlcv::zscore::ZScore;
use crate::snapshot::FeatureSnapshot;

/// Calcula `FeatureSnapshot` a partir de uma sequência de candles **de um
/// único instrumento**, alimentados em ordem cronológica. Não há
/// verificação de instrumento/ordem aqui — assim como
/// `strategies::Strategy::on_event`, é responsabilidade de quem chama
/// (`app::pipeline`, `backtest::BacktestRunner`, ...) manter um
/// `FeatureEngine` por instrumento e alimentá-lo só com candles fechados,
/// em ordem. Ver o doc do crate raiz para a garantia de não-look-ahead.
pub struct FeatureEngine {
    instrument_id: InstrumentId,
    config: FeatureConfig,

    returns: MultiHorizonReturns,
    sma: Sma,
    ema: EmaFeature,
    stddev: PriceStdDev,
    realized_volatility: RealizedVolatility,
    ewma_volatility: EwmaVolatility,
    zscore: ZScore,
    bollinger: Bollinger,
    atr: Atr,
    rsi: Rsi,
    roc: Roc,
    regression: RollingLinearRegression,
    autocorrelation: Autocorrelation,
    relative_volume: RelativeVolume,
}

impl FeatureEngine {
    pub fn new(instrument_id: InstrumentId, config: FeatureConfig) -> Self {
        Self {
            returns: MultiHorizonReturns::new(config.return_periods.clone()),
            sma: Sma::new(config.sma_period),
            ema: EmaFeature::new(config.ema_period),
            stddev: PriceStdDev::new(config.stddev_period),
            realized_volatility: RealizedVolatility::new(config.realized_volatility_period),
            ewma_volatility: EwmaVolatility::new(config.ewma_lambda),
            zscore: ZScore::new(config.zscore_period),
            bollinger: Bollinger::new(config.bollinger_period, config.bollinger_k),
            atr: Atr::new(config.atr_period),
            rsi: Rsi::new(config.rsi_period),
            roc: Roc::new(config.roc_period),
            regression: RollingLinearRegression::new(config.regression_period.max(2)),
            autocorrelation: Autocorrelation::new(
                config.autocorrelation_period,
                config.autocorrelation_lag,
            ),
            relative_volume: RelativeVolume::new(config.relative_volume_period),
            instrument_id,
            config,
        }
    }

    pub fn config(&self) -> &FeatureConfig {
        &self.config
    }

    /// Alimenta um candle fechado e devolve o `FeatureSnapshot` resultante.
    /// `None` só quando `candle.close`/`high`/`low`/`volume` não cabem em
    /// `f64` (na prática, nunca acontece para preços/volumes reais — a
    /// checagem existe pela mesma razão que `strategies::generic` já usa
    /// `Decimal::to_f64()?` nas próprias estratégias, não por um risco
    /// concreto observado).
    pub fn update(&mut self, candle: &Candle) -> Option<FeatureSnapshot> {
        let close = candle.close.to_f64()?;
        let high = candle.high.to_f64()?;
        let low = candle.low.to_f64()?;
        let volume = candle.volume.to_f64()?;

        Some(FeatureSnapshot {
            instrument_id: self.instrument_id,
            timestamp: candle.close_time,
            close,
            returns: self.returns.update(close),
            sma: self.sma.update(close),
            ema: Some(self.ema.update(close)),
            stddev: self.stddev.update(close),
            realized_volatility: self.realized_volatility.update(close),
            ewma_volatility: self.ewma_volatility.update(close),
            zscore: self.zscore.update(close),
            bollinger: self.bollinger.update(close),
            atr: self.atr.update(high, low, close),
            rsi: self.rsi.update(close),
            roc: self.roc.update(close),
            regression: self.regression.update(close),
            autocorrelation: self.autocorrelation.update(close),
            relative_volume: self.relative_volume.update(volume),
        })
    }
}

/// Aplica `FeatureEngine` a uma série histórica **já ordenada
/// cronologicamente** de candles de um único instrumento, devolvendo um
/// snapshot por candle na mesma ordem — o snapshot no índice `i` reflete
/// só `candles[0..=i]`, nunca candles futuros da mesma série (a mesma
/// garantia de `FeatureEngine::update`, aplicada em lote). Útil para
/// pré-calcular features sobre histórico (backtest, pesquisa) sem escrever
/// o loop de alimentação manualmente. Candles com `is_closed == false` são
/// ignorados, e a ordenação **não** é verificada nem reordenada aqui —
/// diferente de `backtest::BacktestRunner::run` (que ordena por
/// `close_time` antes de processar), é responsabilidade de quem chama
/// garantir a ordem.
pub fn compute_series(
    instrument_id: InstrumentId,
    candles: &[Candle],
    config: FeatureConfig,
) -> Vec<FeatureSnapshot> {
    let mut engine = FeatureEngine::new(instrument_id, config);
    candles
        .iter()
        .filter(|c| c.is_closed)
        .filter_map(|c| engine.update(c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use domain::Timeframe;
    use rust_decimal_macros::dec;

    fn candle(minute: i64, close: rust_decimal::Decimal) -> Candle {
        let open_time =
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap() + chrono::Duration::minutes(minute);
        Candle {
            instrument_id: InstrumentId::new(),
            timeframe: Timeframe::M1,
            open_time,
            close_time: open_time + chrono::Duration::minutes(1),
            open: close,
            high: close + dec!(1),
            low: close - dec!(1),
            close,
            volume: dec!(100),
            is_closed: true,
        }
    }

    #[test]
    fn no_look_ahead_snapshot_at_index_i_is_unaffected_by_future_candles() {
        let instrument_id = InstrumentId::new();
        let config = FeatureConfig {
            sma_period: 3,
            ema_period: 3,
            return_periods: vec![1, 2],
            ..FeatureConfig::default()
        };

        let prices: Vec<rust_decimal::Decimal> = [100, 101, 99, 105, 95, 110, 90, 120]
            .iter()
            .map(|p| rust_decimal::Decimal::from(*p))
            .collect();
        let candles: Vec<Candle> = prices
            .iter()
            .enumerate()
            .map(|(i, p)| candle(i as i64, *p))
            .collect();

        // Roda o motor sobre um prefixo curto (5 candles).
        let short_run = compute_series(instrument_id, &candles[..5], config.clone());
        // Roda de novo sobre a série inteira (8 candles).
        let full_run = compute_series(instrument_id, &candles, config);

        // O snapshot no índice 4 (5º candle) deve ser bit-a-bit idêntico
        // nas duas execuções — os 3 candles futuros extras da segunda
        // execução não podem ter alterado nada retroativamente.
        assert_eq!(short_run[4], full_run[4]);
    }

    #[test]
    fn produces_a_snapshot_per_closed_candle_and_skips_open_ones() {
        let instrument_id = InstrumentId::new();
        let mut open_candle = candle(3, dec!(100));
        open_candle.is_closed = false;

        let candles = vec![candle(0, dec!(100)), candle(1, dec!(101)), open_candle];
        let snapshots = compute_series(instrument_id, &candles, FeatureConfig::default());
        assert_eq!(snapshots.len(), 2);
    }

    #[test]
    fn ema_is_always_present_from_the_first_candle() {
        let instrument_id = InstrumentId::new();
        let mut engine = FeatureEngine::new(instrument_id, FeatureConfig::default());
        let snapshot = engine.update(&candle(0, dec!(100))).unwrap();
        assert_eq!(snapshot.ema, Some(100.0));
        assert_eq!(snapshot.sma, None); // sma_period default = 20, ainda não encheu
    }
}
