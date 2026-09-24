use thiserror::Error;

#[derive(Debug, Error)]
pub enum MarketDataError {
    #[error("http request to {url} failed: {source}")]
    Http {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("exchange returned an error response: {0}")]
    ExchangeError(String),
    #[error("failed to parse exchange payload: {0}")]
    Parse(String),
    #[error("websocket connection failed: {0}")]
    WebSocket(String),
    #[error("instrument {0} not found on exchange")]
    InstrumentNotFound(String),
}
