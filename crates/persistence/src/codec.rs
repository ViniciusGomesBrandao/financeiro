//! Codificação em string dos enums pequenos do domínio, compartilhada por
//! todos os módulos de repositório. Mantida fora de `domain` de propósito:
//! como um enum é escrito no Postgres é uma preocupação da camada de
//! persistência, não do domínio.

use domain::{
    AssetClass, Liquidity, MarketDataKind, MarketType, OrderStatus, OrderType, Side,
    SignalDirection,
};
use sqlx::Error as SqlxError;

use crate::error::PersistenceError;

fn decode_err(field: &str, value: &str) -> PersistenceError {
    PersistenceError::Database(SqlxError::Decode(
        format!("unknown {field} {value:?}").into(),
    ))
}

pub fn side_to_str(v: Side) -> &'static str {
    match v {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

pub fn side_from_str(v: &str) -> Result<Side, PersistenceError> {
    match v {
        "buy" => Ok(Side::Buy),
        "sell" => Ok(Side::Sell),
        other => Err(decode_err("side", other)),
    }
}

pub fn order_type_to_str(v: OrderType) -> &'static str {
    match v {
        OrderType::Market => "market",
        OrderType::Limit => "limit",
    }
}

pub fn order_type_from_str(v: &str) -> Result<OrderType, PersistenceError> {
    match v {
        "market" => Ok(OrderType::Market),
        "limit" => Ok(OrderType::Limit),
        other => Err(decode_err("order_type", other)),
    }
}

pub fn order_status_to_str(v: OrderStatus) -> &'static str {
    match v {
        OrderStatus::New => "new",
        OrderStatus::PartiallyFilled => "partially_filled",
        OrderStatus::Filled => "filled",
        OrderStatus::Rejected => "rejected",
        OrderStatus::Cancelled => "cancelled",
    }
}

pub fn order_status_from_str(v: &str) -> Result<OrderStatus, PersistenceError> {
    match v {
        "new" => Ok(OrderStatus::New),
        "partially_filled" => Ok(OrderStatus::PartiallyFilled),
        "filled" => Ok(OrderStatus::Filled),
        "rejected" => Ok(OrderStatus::Rejected),
        "cancelled" => Ok(OrderStatus::Cancelled),
        other => Err(decode_err("order_status", other)),
    }
}

pub fn liquidity_to_str(v: Liquidity) -> &'static str {
    match v {
        Liquidity::Maker => "maker",
        Liquidity::Taker => "taker",
    }
}

pub fn liquidity_from_str(v: &str) -> Result<Liquidity, PersistenceError> {
    match v {
        "maker" => Ok(Liquidity::Maker),
        "taker" => Ok(Liquidity::Taker),
        other => Err(decode_err("liquidity", other)),
    }
}

pub fn signal_direction_to_str(v: SignalDirection) -> &'static str {
    match v {
        SignalDirection::Long => "long",
        SignalDirection::Short => "short",
        SignalDirection::Flat => "flat",
    }
}

pub fn signal_direction_from_str(v: &str) -> Result<SignalDirection, PersistenceError> {
    match v {
        "long" => Ok(SignalDirection::Long),
        "short" => Ok(SignalDirection::Short),
        "flat" => Ok(SignalDirection::Flat),
        other => Err(decode_err("signal_direction", other)),
    }
}

pub fn position_status_to_str(v: domain::PositionStatus) -> &'static str {
    match v {
        domain::PositionStatus::Open => "open",
        domain::PositionStatus::Closed => "closed",
    }
}

pub fn position_status_from_str(v: &str) -> Result<domain::PositionStatus, PersistenceError> {
    match v {
        "open" => Ok(domain::PositionStatus::Open),
        "closed" => Ok(domain::PositionStatus::Closed),
        other => Err(decode_err("position_status", other)),
    }
}

pub fn asset_class_to_str(v: AssetClass) -> &'static str {
    match v {
        AssetClass::Crypto => "crypto",
        AssetClass::CryptoDerivative => "crypto_derivative",
        AssetClass::Equity => "equity",
        AssetClass::Future => "future",
        AssetClass::Forex => "forex",
    }
}

pub fn asset_class_from_str(v: &str) -> Result<AssetClass, PersistenceError> {
    match v {
        "crypto" => Ok(AssetClass::Crypto),
        "crypto_derivative" => Ok(AssetClass::CryptoDerivative),
        "equity" => Ok(AssetClass::Equity),
        "future" => Ok(AssetClass::Future),
        "forex" => Ok(AssetClass::Forex),
        other => Err(decode_err("asset_class", other)),
    }
}

pub fn market_type_to_str(v: MarketType) -> &'static str {
    match v {
        MarketType::Spot => "spot",
        MarketType::Perpetual => "perpetual",
        MarketType::DatedFuture => "dated_future",
        MarketType::Equity => "equity",
    }
}

pub fn market_type_from_str(v: &str) -> Result<MarketType, PersistenceError> {
    match v {
        "spot" => Ok(MarketType::Spot),
        "perpetual" => Ok(MarketType::Perpetual),
        "dated_future" => Ok(MarketType::DatedFuture),
        "equity" => Ok(MarketType::Equity),
        other => Err(decode_err("market_type", other)),
    }
}

pub fn market_data_kind_to_str(v: MarketDataKind) -> &'static str {
    match v {
        MarketDataKind::Ohlcv => "ohlcv",
        MarketDataKind::Trades => "trades",
        MarketDataKind::OrderBookL1 => "order_book_l1",
        MarketDataKind::OrderBookL2 => "order_book_l2",
        MarketDataKind::FundingRate => "funding_rate",
        MarketDataKind::OpenInterest => "open_interest",
        MarketDataKind::TradingSession => "trading_session",
        MarketDataKind::OnChain => "on_chain",
    }
}

pub fn market_data_kind_from_str(v: &str) -> Result<MarketDataKind, PersistenceError> {
    match v {
        "ohlcv" => Ok(MarketDataKind::Ohlcv),
        "trades" => Ok(MarketDataKind::Trades),
        "order_book_l1" => Ok(MarketDataKind::OrderBookL1),
        "order_book_l2" => Ok(MarketDataKind::OrderBookL2),
        "funding_rate" => Ok(MarketDataKind::FundingRate),
        "open_interest" => Ok(MarketDataKind::OpenInterest),
        "trading_session" => Ok(MarketDataKind::TradingSession),
        "on_chain" => Ok(MarketDataKind::OnChain),
        other => Err(decode_err("market_data_kind", other)),
    }
}
