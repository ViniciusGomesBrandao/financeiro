use std::fmt;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::error::DomainError;
use crate::instrument::InstrumentId;
use crate::timeframe::Timeframe;

/// Identifica uma configuração de estratégia (não uma *instância* de estratégia
/// — um mesmo `StrategyId` pode rodar concorrentemente sobre vários
/// instrumentos).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StrategyId(String);

impl StrategyId {
    pub fn new(id: impl Into<String>) -> Result<Self, DomainError> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err(DomainError::EmptyStrategyId);
        }
        Ok(Self(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StrategyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SignalId(pub Uuid);

impl SignalId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SignalId {
    fn default() -> Self {
        Self::new()
    }
}

/// A intenção direcional expressa por um sinal. `Flat` significa "fechar /
/// ficar de fora" — não é, em si, uma posição short.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SignalDirection {
    Long,
    Short,
    Flat,
}

/// O resultado da avaliação de uma estratégia: uma opinião sobre um instrumento,
/// não uma instrução para negociá-lo. Um `Signal` deliberadamente *não* é uma
/// `Order` — não carrega quantidade nem tipo de ordem, e não foi verificado
/// contra limites de risco ou estado da conta. Somente o motor de risco pode
/// transformar um sinal em uma intenção de ordem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    pub id: SignalId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub direction: SignalDirection,
    /// Confiança autodeclarada pela estratégia em `[0.0, 1.0]`. Não é garantia
    /// de probabilidade — é puramente uma dica de ponderação relativa para
    /// dimensionamento de risco e analytics.
    pub confidence: f64,
    pub timestamp: DateTime<Utc>,
    /// Retorno esperado opcional ao longo de `time_horizon`, quando a estratégia
    /// consegue estimá-lo. Nunca invente este valor se a estratégia não tiver
    /// base para ele — deixe como `None`.
    pub expected_return: Option<Decimal>,
    pub time_horizon: Option<Timeframe>,
    /// Contexto livre específico da estratégia (valores de indicadores,
    /// z-scores, etc.) para logging/analytics. Não é interpretado pelo risco nem
    /// pela execução.
    pub metadata: Value,
}

impl Signal {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        direction: SignalDirection,
        confidence: f64,
        timestamp: DateTime<Utc>,
        expected_return: Option<Decimal>,
        time_horizon: Option<Timeframe>,
        metadata: Value,
    ) -> Result<Self, DomainError> {
        if !(0.0..=1.0).contains(&confidence) {
            return Err(DomainError::ConfidenceOutOfRange(confidence));
        }
        Ok(Self {
            id: SignalId::new(),
            strategy_id,
            instrument_id,
            direction,
            confidence,
            timestamp,
            expected_return,
            time_horizon,
            metadata,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_confidence_out_of_range() {
        let result = Signal::new(
            StrategyId::new("ema-crossover").unwrap(),
            InstrumentId::new(),
            SignalDirection::Long,
            1.5,
            Utc::now(),
            None,
            None,
            Value::Null,
        );
        assert!(result.is_err());
    }

    #[test]
    fn strategy_id_rejects_empty() {
        assert!(StrategyId::new("").is_err());
        assert!(StrategyId::new("  ").is_err());
    }
}
