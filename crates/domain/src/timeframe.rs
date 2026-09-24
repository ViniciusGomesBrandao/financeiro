use std::fmt;

use chrono::Duration;
use serde::{Deserialize, Serialize};

/// Um período de agregação de candles. Deliberadamente um enum fechado (não uma
/// duração crua) para que adaptadores e estratégias compartilhem um único
/// vocabulário para "candle de 1 minuto" em vez de comparar `Duration`s cruas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Timeframe {
    M1,
    M5,
    M15,
    M30,
    H1,
    H4,
    D1,
    W1,
}

impl Timeframe {
    pub fn duration(&self) -> Duration {
        match self {
            Timeframe::M1 => Duration::minutes(1),
            Timeframe::M5 => Duration::minutes(5),
            Timeframe::M15 => Duration::minutes(15),
            Timeframe::M30 => Duration::minutes(30),
            Timeframe::H1 => Duration::hours(1),
            Timeframe::H4 => Duration::hours(4),
            Timeframe::D1 => Duration::days(1),
            Timeframe::W1 => Duration::weeks(1),
        }
    }

    /// O rótulo de transmissão agnóstico de exchange. O mapeamento específico da
    /// Binance (ex. `"1m"`) fica no adaptador de market-data, não aqui.
    pub fn as_str(&self) -> &'static str {
        match self {
            Timeframe::M1 => "1m",
            Timeframe::M5 => "5m",
            Timeframe::M15 => "15m",
            Timeframe::M30 => "30m",
            Timeframe::H1 => "1h",
            Timeframe::H4 => "4h",
            Timeframe::D1 => "1d",
            Timeframe::W1 => "1w",
        }
    }

    /// Parseia o rótulo canônico (`1m`, `15m`, `1h`, ...) usado no dashboard
    /// e em `operational_robots.timeframe`. Aceita também aliases curtos
    /// (`M15`, `H1`) por conveniência em testes/config.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "1m" | "m1" => Some(Self::M1),
            "5m" | "m5" => Some(Self::M5),
            "15m" | "m15" => Some(Self::M15),
            "30m" | "m30" => Some(Self::M30),
            "1h" | "h1" => Some(Self::H1),
            "4h" | "h4" => Some(Self::H4),
            "1d" | "d1" => Some(Self::D1),
            "1w" | "w1" => Some(Self::W1),
            _ => None,
        }
    }
}

impl fmt::Display for Timeframe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_matches_label() {
        assert_eq!(Timeframe::M1.duration(), Duration::minutes(1));
        assert_eq!(Timeframe::H4.duration(), Duration::hours(4));
        assert_eq!(Timeframe::D1.as_str(), "1d");
    }

    #[test]
    fn parse_accepts_canonical_labels() {
        assert_eq!(Timeframe::parse("15m"), Some(Timeframe::M15));
        assert_eq!(Timeframe::parse("1h"), Some(Timeframe::H1));
        assert_eq!(Timeframe::parse("M1"), Some(Timeframe::M1));
        assert_eq!(Timeframe::parse("nope"), None);
    }
}
