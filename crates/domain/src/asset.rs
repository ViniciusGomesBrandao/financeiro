use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// Código de um ativo/moeda base ou de cotação, ex. `BTC`, `USDT`, `AAPL`.
///
/// Sempre normalizado para maiúsculas. Existe para que códigos de ativos sejam
/// validados uma única vez na fronteira, em vez de circularem como `String`s
/// cruas.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Asset(String);

impl Asset {
    pub fn new(code: impl AsRef<str>) -> Result<Self, DomainError> {
        let code = code.as_ref().trim();
        if code.is_empty() || !code.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(DomainError::InvalidAsset(code.to_string()));
        }
        Ok(Self(code.to_ascii_uppercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Asset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Identificador legível e agnóstico de exchange de um par de negociação,
/// ex. `BTC/USDT`.
///
/// Este *não* é, deliberadamente, um símbolo nativo de exchange como `BTCUSDT`
/// ou `BTC-USDT`: esses formatos são detalhes de implementação específicos da
/// Binance/Coinbase que nunca devem vazar da camada de adaptadores de
/// market-data para fora.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Symbol(String);

impl Symbol {
    pub fn from_pair(base: &Asset, quote: &Asset) -> Self {
        Self(format!("{base}/{quote}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_to_uppercase() {
        let asset = Asset::new("btc").unwrap();
        assert_eq!(asset.as_str(), "BTC");
    }

    #[test]
    fn rejects_empty_or_invalid() {
        assert!(Asset::new("").is_err());
        assert!(Asset::new("BT C").is_err());
        assert!(Asset::new("BTC-USDT").is_err());
    }

    #[test]
    fn symbol_formats_as_base_slash_quote() {
        let base = Asset::new("BTC").unwrap();
        let quote = Asset::new("USDT").unwrap();
        assert_eq!(Symbol::from_pair(&base, &quote).as_str(), "BTC/USDT");
    }
}
