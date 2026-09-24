use thiserror::Error;

#[derive(Debug, Error)]
pub enum BacktestError {
    #[error("invalid mark price encountered during replay: {0}")]
    InvalidPrice(#[from] domain::DomainError),
    #[error("broker error during replay: {0}")]
    Broker(#[from] execution::BrokerError),
}
