use domain::OrderType;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BrokerError {
    #[error("unsupported order type {0:?}: PaperBroker only executes Market orders in this phase")]
    UnsupportedOrderType(OrderType),
    #[error("invalid order quantity")]
    InvalidQuantity,
    #[error("computed execution price is invalid (reference price + slippage <= 0)")]
    InvalidExecutionPrice,
    #[error(
        "order side matches an already-open position for this instrument; \
         the risk engine should never have approved this (no pyramiding support)"
    )]
    UnexpectedSameSideOrder,
    #[error(
        "cannot sell an asset this spot account does not hold (naked short); \
         the risk engine should never have approved this"
    )]
    NakedShortNotSupported,
    #[error(transparent)]
    Portfolio(#[from] portfolio::PortfolioError),
}
