use thiserror::Error;

/// Erros de validação de invariantes do domínio. São construídos exclusivamente
/// pelos construtores `try_new` / `new` que protegem a invariante que nomeiam.
#[derive(Debug, Error, PartialEq)]
pub enum DomainError {
    #[error("asset code must be non-empty uppercase alphanumeric, got {0:?}")]
    InvalidAsset(String),

    #[error("price must be positive, got {0}")]
    NonPositivePrice(String),

    #[error("quantity must be positive, got {0}")]
    NonPositiveQuantity(String),

    #[error("confidence must be within [0.0, 1.0], got {0}")]
    ConfidenceOutOfRange(f64),

    #[error("strategy id must be non-empty")]
    EmptyStrategyId,
}
