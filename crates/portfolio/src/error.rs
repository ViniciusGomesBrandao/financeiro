use domain::{InstrumentId, StrategyId};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PortfolioError {
    #[error("no open position for instrument {instrument_id:?} and strategy {strategy_id}")]
    NoOpenPosition {
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
    },
    /// Os dados carregados do Postgres para reconstruir o `PortfolioManager`
    /// no bootstrap violam uma invariante que o motor de risco depende
    /// (ex.: mais de uma posição aberta para o mesmo instrumento, ou uma
    /// posição marcada como aberta/fechada com o status errado). Em vez de
    /// silenciosamente escolher uma das posições ou ignorar a
    /// inconsistência, o restart falha explicitamente — ver
    /// `PortfolioManager::restore`.
    #[error("inconsistent state while restoring portfolio: {0}")]
    InconsistentRestore(String),
}
