//! Resultado da seleção contextual (regime + saúde econômica).

use domain::StrategyId;

use crate::decision::{JudgeDecision, JudgeState};
use crate::regime::{MarketRegime, RegimeAssessment};

/// Motivo estruturado da escolha da estratégia ativa.
#[derive(Debug, Clone, PartialEq)]
pub enum SelectionReason {
    /// Escolhida pela afinidade com o regime observado.
    RegimeFit {
        regime: MarketRegime,
        strategy_kind: String,
        fit_score: f64,
        summary: String,
    },
    /// Regime incerto / sem features — caiu no ranking econômico.
    EconomicFallback { summary: String },
    /// Sem histórico de trades; bootstrap pela afinidade de regime.
    RegimeBootstrap {
        regime: MarketRegime,
        strategy_kind: String,
        fit_score: f64,
        summary: String,
    },
    /// Histerese: a proposta ainda não acumulou confirmações.
    SelectionPending {
        proposed: Option<StrategyId>,
        confirmations: u32,
        required: u32,
        summary: String,
    },
    /// Nenhuma candidata viável.
    NoViableCandidate { summary: String },
}

/// Afinidade de uma candidata ao regime atual (para dashboard).
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateFit {
    pub strategy_id: StrategyId,
    pub strategy_kind: String,
    pub fit_score: f64,
    pub economic_state: JudgeState,
}

/// Pacote completo devolvido por `StrategyJudge::recommend_contextual`.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionOutcome {
    pub decisions: Vec<JudgeDecision>,
    pub selected: Option<StrategyId>,
    pub regime: RegimeAssessment,
    pub selection_reason: SelectionReason,
    pub candidate_fits: Vec<CandidateFit>,
}
