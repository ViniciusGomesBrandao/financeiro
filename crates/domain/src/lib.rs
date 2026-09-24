//! Tipos de domínio agnósticos de exchange e de broker, compartilhados por
//! todos os outros crates.
//!
//! Este crate tem zero dependências de infraestrutura (sem tokio, sem sqlx,
//! sem cliente HTTP) por design: é puramente dados + invariantes. Tudo que
//! precisa de I/O, persistência ou parsing específico de exchange pertence a
//! um crate de nível mais alto (`market-data`, `persistence`, `execution`, ...).
//!
//! Veja `docs/architecture.md` e o `README.md` na raiz do repositório para os
//! princípios arquiteturais que este crate existe para garantir, em especial:
//! `Signal != Order`, e `AssetClass` deve permanecer um enum pequeno e universal.

pub mod asset;
pub mod asset_class;
pub mod calendar;
pub mod error;
pub mod instrument;
pub mod market_data;
pub mod market_event;
pub mod money;
pub mod order;
pub mod portfolio_snapshot;
pub mod position;
pub mod side;
pub mod signal;
pub mod timeframe;

pub use asset::{Asset, Symbol};
pub use asset_class::{AssetClass, Exchange, MarketDataKind, MarketType};
pub use calendar::{AlwaysOpenCalendar, TradingCalendar};
pub use error::DomainError;
pub use instrument::{Instrument, InstrumentId};
pub use market_data::{
    BookLevel, BookTicker, Candle, MarketTrade, OrderBookDelta, OrderBookSnapshot,
};
pub use market_event::MarketEvent;
pub use money::{notional, Money, Price, Quantity};
pub use order::{Fill, Liquidity, Order, OrderRequest, OrderStatus, OrderType};
pub use portfolio_snapshot::PortfolioSnapshot;
pub use position::{Position, PositionStatus};
pub use side::Side;
pub use signal::{Signal, SignalDirection, SignalId, StrategyId};
pub use timeframe::Timeframe;
