//! Estado de features por robô para o Judge contextual (Fase 2).
//! Um `FeatureEngine` por robô, alimentado só com candles do timeframe
//! daquele robô — sem look-ahead.

use std::collections::HashMap;

use domain::{Candle, InstrumentId};
use features::{FeatureConfig, FeatureEngine, FeatureSnapshot};

/// Visão de mercado acumulada por robô para classificação de regime.
pub struct RobotMarketView {
    engine: FeatureEngine,
    prev_bandwidth: Option<f64>,
    /// Último snapshot produzido (para seed do Judge após warm-up).
    last_snapshot: Option<FeatureSnapshot>,
    /// `prev_bandwidth` que acompanhou `last_snapshot` (pré-candle).
    last_judge_prev_bandwidth: Option<f64>,
}

impl RobotMarketView {
    pub fn new(instrument_id: InstrumentId) -> Self {
        Self {
            engine: FeatureEngine::new(instrument_id, FeatureConfig::default()),
            prev_bandwidth: None,
            last_snapshot: None,
            last_judge_prev_bandwidth: None,
        }
    }

    /// Alimenta um candle fechado. Devolve `(snapshot, prev_bandwidth)`
    /// quando o engine produz um snapshot; `prev_bandwidth` é o valor
    /// *antes* deste candle (para detectar expansão sem look-ahead).
    pub fn on_candle(&mut self, candle: &Candle) -> Option<(FeatureSnapshot, Option<f64>)> {
        if !candle.is_closed {
            return None;
        }
        let snapshot = self.engine.update(candle)?;
        let prev = self.prev_bandwidth;
        self.prev_bandwidth = snapshot.bollinger.map(|b| b.bandwidth);
        self.last_snapshot = Some(snapshot.clone());
        self.last_judge_prev_bandwidth = prev;
        Some((snapshot, prev))
    }

    /// Snapshot mais recente + `prev_bandwidth` associado (warm-up / seed).
    pub fn last_for_judge(&self) -> Option<(&FeatureSnapshot, Option<f64>)> {
        self.last_snapshot
            .as_ref()
            .map(|snap| (snap, self.last_judge_prev_bandwidth))
    }
}

/// Cria uma view vazia por robô a partir dos contextos operacionais.
pub fn build_robot_market_views(
    robots: &[crate::robot_runtime::RobotContext],
) -> HashMap<String, RobotMarketView> {
    robots
        .iter()
        .map(|r| (r.id.clone(), RobotMarketView::new(r.instrument_id)))
        .collect()
}
