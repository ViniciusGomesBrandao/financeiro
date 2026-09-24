use std::fmt;

use serde::{Deserialize, Serialize};

/// A categoria ampla de mercado à qual um instrumento pertence.
///
/// É o ponto de junção que as estratégias usam para declarar compatibilidade.
/// Mantenha-o pequeno e universal — ele nunca deve ganhar campos específicos de
/// mercado (ex. nada de `tick_size` aqui). Tudo que difere de forma relevante
/// entre cripto e ações pertence a `Instrument` ou a módulos específicos de
/// mercado, não a este enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AssetClass {
    /// Criptomoeda no mercado spot (ex. BTC/USDT num livro de ofertas spot).
    Crypto,
    /// Derivativos de cripto: futuros perpétuos, futuros com vencimento, opções.
    /// Mantido separado de `Crypto` porque habilita dados que as estratégias não
    /// podem assumir existentes para instrumentos spot (funding rate, open
    /// interest).
    CryptoDerivative,
    /// Ação listada (ações, ETFs).
    Equity,
    /// Contratos futuros tradicionais (não-cripto).
    Future,
    /// Câmbio spot/a termo.
    Forex,
}

impl fmt::Display for AssetClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            AssetClass::Crypto => "Crypto",
            AssetClass::CryptoDerivative => "CryptoDerivative",
            AssetClass::Equity => "Equity",
            AssetClass::Future => "Future",
            AssetClass::Forex => "Forex",
        };
        write!(f, "{s}")
    }
}

/// O local (venue) onde um instrumento é negociado.
///
/// `Other` existe para que novos venues possam ser plugados sem mexer em todos
/// os pontos de uso deste enum; ainda assim é necessário um adaptador de
/// market-data para efetivamente obter dados desse venue.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Exchange {
    Binance,
    Other(String),
}

impl fmt::Display for Exchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Exchange::Binance => write!(f, "Binance"),
            Exchange::Other(name) => write!(f, "{name}"),
        }
    }
}

/// Como um instrumento é negociado em sua exchange (estrutura do livro de
/// ofertas), em contraste com sua classe de ativo. Uma classe `Crypto` pode ser
/// `Spot` ou (como `CryptoDerivative`) `Perpetual` / `DatedFuture`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MarketType {
    Spot,
    Perpetual,
    DatedFuture,
    Equity,
}

impl fmt::Display for MarketType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            MarketType::Spot => "Spot",
            MarketType::Perpetual => "Perpetual",
            MarketType::DatedFuture => "DatedFuture",
            MarketType::Equity => "Equity",
        };
        write!(f, "{s}")
    }
}

/// Um tipo de dado de mercado que um provedor pode fornecer e uma estratégia
/// pode exigir.
///
/// É o vocabulário em que as capacidades de `StrategyRequirements` e
/// `MarketDataProvider` são expressas. Adicione uma variante somente quando um
/// provedor ou estratégia real precisar dela — não crie feeds especulativos
/// antecipadamente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MarketDataKind {
    Ohlcv,
    Trades,
    OrderBookL1,
    OrderBookL2,
    FundingRate,
    OpenInterest,
    TradingSession,
    OnChain,
}

impl fmt::Display for MarketDataKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            MarketDataKind::Ohlcv => "OHLCV",
            MarketDataKind::Trades => "Trades",
            MarketDataKind::OrderBookL1 => "OrderBookL1",
            MarketDataKind::OrderBookL2 => "OrderBookL2",
            MarketDataKind::FundingRate => "FundingRate",
            MarketDataKind::OpenInterest => "OpenInterest",
            MarketDataKind::TradingSession => "TradingSession",
            MarketDataKind::OnChain => "OnChain",
        };
        write!(f, "{s}")
    }
}
