use domain::{AssetClass, MarketDataKind};
use thiserror::Error;

/// Por que uma estratégia não pôde ser registrada contra um instrumento.
/// Retornado imediatamente no momento do registro — nunca descoberto depois como
/// um no-op silencioso ou um panic em tempo de execução.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CompatibilityError {
    #[error(
        "strategy {strategy_id} does not support asset class {asset_class} (instrument {symbol})"
    )]
    UnsupportedAssetClass {
        strategy_id: String,
        symbol: String,
        asset_class: AssetClass,
    },

    #[error("strategy {strategy_id} requires {missing:?} which is not available for {symbol}")]
    MissingMarketData {
        strategy_id: String,
        symbol: String,
        missing: MarketDataKind,
    },
}
