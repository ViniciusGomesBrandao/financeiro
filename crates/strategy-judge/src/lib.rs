//! Strategy Judge (Fase 2 contextual): classifica o regime de mercado a
//! partir de features OHLCV, avalia a saúde econômica das candidatas e
//! recomenda qual estratégia do robô deve operar.
//!
//! **O que o Judge faz.**
//! 1. Lê `FeatureSnapshot` (quando disponível) e classifica o regime
//!    (`Trending` / `Ranging` / `VolatilityExpansion` / `Uncertain`).
//! 2. Avalia cada candidata via histórico de trades fechados
//!    (`analytics::PerformanceReport` + `JudgeThresholds`).
//! 3. Combina afinidade regime×kind com a saúde econômica e recomenda
//!    uma estratégia — com histerese (`min_confirmations`) contra churn.
//!
//! **O que o Judge nunca faz** — isolamento deliberado:
//! - Não depende de `strategies`, `portfolio`, `risk`, `execution` nem
//!   `persistence`. Depende de `domain`, `analytics` e `features`.
//! - Não instancia nem registra estratégias — os `StrategyId`s vêm do
//!   chamador (candidatas do robô).
//! - Não envia ordens; devolve `SelectionOutcome` / `JudgeDecision`.
//!
//! **Sem look-ahead.** `evaluate` filtra `closed_at <= as_of`. Features
//! devem ser alimentadas só com candles já fechados até `as_of`.
//!
//! **Comparação só entre candidatas do mesmo contexto (robô).**

pub mod affinity;
pub mod decision;
pub mod judge;
pub mod regime;
pub mod selection;
pub mod thresholds;

pub use affinity::{regime_affinity, strategy_kind_from_id};
pub use decision::{JudgeDecision, JudgeMetrics, JudgeReason, JudgeState};
pub use judge::StrategyJudge;
pub use regime::{
    classify_regime, MarketRegime, RegimeAssessment, RegimeEvidence, RegimeThresholds,
};
pub use selection::{CandidateFit, SelectionOutcome, SelectionReason};
pub use thresholds::JudgeThresholds;
