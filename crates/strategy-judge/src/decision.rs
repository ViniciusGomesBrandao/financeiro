use chrono::{DateTime, Utc};
use domain::{InstrumentId, StrategyId};
use rust_decimal::Decimal;

/// O veredito do Judge sobre uma `(StrategyId, InstrumentId)`: uma
/// recomendação de operação, nunca uma ordem nem uma mudança de
/// parâmetros. `Degraded` existe entre os dois extremos para representar
/// "ainda economicamente aceitável, mas não a melhor escolha nem
/// claramente saudável" — sem isso, qualquer reprovação parcial cairia
/// abruptamente em `Disabled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JudgeState {
    /// Passa em todos os critérios configurados — apta a operar.
    Active,
    /// Reprova em exatamente um critério, ou está numa transição ainda
    /// não confirmada (ver `JudgeReason::TransitionPending`) — continua
    /// operável, mas sinaliza atenção.
    Degraded,
    /// Amostra insuficiente para opinar, ou reprova em múltiplos
    /// critérios / tem expectancy não-positiva — não recomendada.
    Disabled,
}

/// Por que o Judge chegou a este `JudgeState` — sempre autoexplicativo,
/// nunca um rótulo seco (mesmo espírito de `risk::RejectionReason`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum JudgeReason {
    /// Menos trades fechados do que `JudgeThresholds::min_sample_size` —
    /// não há evidência suficiente para opinar sobre viabilidade.
    InsufficientSample { trades: usize, required: usize },
    /// Passou em todos os critérios configurados.
    MeetsAllThresholds,
    /// `win_rate` (a métrica de consistência) abaixo do mínimo.
    BelowConsistencyThreshold { win_rate: Decimal, min: Decimal },
    /// `profit_factor` abaixo do mínimo. `None` nunca produz este motivo
    /// (ausência de perdedores não é reprovação — ver `judge::classify`).
    BelowProfitFactorThreshold {
        profit_factor: Decimal,
        min: Decimal,
    },
    /// `expectancy` (ganho médio esperado por trade) zero ou negativa —
    /// o critério mais diretamente ligado a "inviável economicamente".
    NonPositiveExpectancy { expectancy: Decimal },
    /// `max_drawdown` acima do máximo configurado.
    ExcessiveDrawdown { drawdown: Decimal, max: Decimal },
    /// A classificação bruta desta avaliação difere do estado
    /// memorizado da última decisão, mas ainda não se repetiu por
    /// `required` avaliações consecutivas — o estado retornado continua
    /// sendo o anterior, não `proposed`. É este motivo que impede uma
    /// pequena sequência negativa de trocar a estratégia imediatamente.
    TransitionPending {
        proposed: JudgeState,
        confirmations: u32,
        required: u32,
    },
}

/// As métricas usadas nesta decisão, sempre derivadas de
/// `analytics::compute_performance` sobre o histórico filtrado a
/// `(strategy_id, instrument_id)` e `closed_at <= timestamp` — nunca
/// recalculadas com uma fórmula própria do Judge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JudgeMetrics {
    pub trades: usize,
    /// "Retorno líquido" — `PerformanceReport::net_pnl` acumulado da
    /// amostra.
    pub net_pnl: Decimal,
    pub expectancy: Decimal,
    pub profit_factor: Option<Decimal>,
    pub max_drawdown: Decimal,
    /// "Consistência" — `PerformanceReport::win_rate`.
    pub win_rate: Decimal,
}

/// Uma decisão completa do Judge, autocontida o suficiente para ser
/// registrada (log ou, futuramente, persistência) sem precisar consultar
/// mais nada: quando (`timestamp`, o instante `as_of` da avaliação — nunca
/// o instante em que o Judge rodou, para preservar a garantia de ausência
/// de look-ahead), quem (`strategy_id`, `instrument_id`), o quê
/// (`state`), com base em quê (`metrics`) e por quê (`reason`).
#[derive(Debug, Clone, PartialEq)]
pub struct JudgeDecision {
    pub timestamp: DateTime<Utc>,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub state: JudgeState,
    pub metrics: JudgeMetrics,
    pub reason: JudgeReason,
}
