use rust_decimal::Decimal;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing required environment variable {0}")]
    Missing(&'static str),
    #[error("invalid value for {name}: {value:?} ({reason})")]
    Invalid {
        name: &'static str,
        value: String,
        reason: String,
    },
}

/// Um par de ativos base/quote lido de `ENABLED_SYMBOLS`, por exemplo
/// `BTC/USDT`.
#[derive(Debug, Clone)]
pub struct SymbolPair {
    pub base: String,
    pub quote: String,
}

/// Uma instância de estratégia lida de `STRATEGY_INSTANCES` — dado cru,
/// ainda não validado como `strategies::StrategyInstanceConfig` (`id` só
/// vira um `StrategyId` de verdade em `setup::build_strategy_registry`,
/// onde o erro de validação tem contexto para reportar). `symbols` usa o
/// mesmo formato `"BASE/QUOTE"` que `Instrument::symbol` produz, para casar
/// diretamente sem nenhuma tradução.
#[derive(Debug, Clone)]
pub struct StrategyInstanceSpec {
    pub id: String,
    pub kind: String,
    pub symbols: Vec<String>,
}

/// Configuração da aplicação, carregada uma única vez na inicialização a
/// partir de variáveis de ambiente (ver `.env.example` para a lista
/// completa e os valores padrão). Nada aqui contém segredo ou credencial
/// hardcoded; `DATABASE_URL` é a única variável obrigatória.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub paper_initial_balance: Decimal,
    pub paper_maker_fee: Decimal,
    pub paper_taker_fee: Decimal,
    pub paper_spread_bps: Decimal,
    pub paper_slippage_bps: Decimal,
    pub enabled_symbols: Vec<SymbolPair>,
    pub enabled_strategies: Vec<String>,
    /// Instâncias explícitas de estratégia (Fase 1.5 — Multi-Strategy
    /// Runner), lidas de `STRATEGY_INSTANCES`. Vazio por padrão: quando
    /// vazio, `setup::build_strategy_registry` cai de volta no
    /// comportamento anterior (uma instância por kind de
    /// `enabled_strategies`, id igual ao kind, aplicada a todo instrumento
    /// compatível por classe de ativo) — nenhum deploy existente muda de
    /// comportamento só por esta fase existir. Ver [§7c/7d do README] para
    /// o formato.
    pub strategy_instances: Vec<StrategyInstanceSpec>,
    pub risk_order_notional: Decimal,
    pub risk_max_position_notional: Decimal,
    pub risk_max_total_exposure: Decimal,
    pub risk_max_open_positions: usize,
    pub risk_stop_loss_pct: Option<Decimal>,
    pub risk_take_profit_pct: Option<Decimal>,
    pub risk_max_daily_loss: Decimal,
    pub warmup_candles: u32,
}

fn env_var(name: &'static str) -> Result<String, ConfigError> {
    std::env::var(name).map_err(|_| ConfigError::Missing(name))
}

fn env_var_or(name: &'static str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn parse_decimal(name: &'static str, raw: &str) -> Result<Decimal, ConfigError> {
    Decimal::from_str(raw).map_err(|e| ConfigError::Invalid {
        name,
        value: raw.to_string(),
        reason: e.to_string(),
    })
}

fn parse_optional_decimal(name: &'static str, raw: &str) -> Result<Option<Decimal>, ConfigError> {
    if raw.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(parse_decimal(name, raw)?))
}

fn parse_usize(name: &'static str, raw: &str) -> Result<usize, ConfigError> {
    raw.parse::<usize>().map_err(|e| ConfigError::Invalid {
        name,
        value: raw.to_string(),
        reason: e.to_string(),
    })
}

fn parse_u32(name: &'static str, raw: &str) -> Result<u32, ConfigError> {
    raw.parse::<u32>().map_err(|e| ConfigError::Invalid {
        name,
        value: raw.to_string(),
        reason: e.to_string(),
    })
}

fn parse_symbols(raw: &str) -> Result<Vec<SymbolPair>, ConfigError> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|pair| {
            let (base, quote) = pair.split_once('/').ok_or_else(|| ConfigError::Invalid {
                name: "ENABLED_SYMBOLS",
                value: pair.to_string(),
                reason: "expected BASE/QUOTE, e.g. BTC/USDT".to_string(),
            })?;
            Ok(SymbolPair {
                base: base.to_string(),
                quote: quote.to_string(),
            })
        })
        .collect()
}

fn parse_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// `STRATEGY_INSTANCES` — instâncias separadas por `;`, cada uma no formato
/// `id:kind:SYMBOLO[+SYMBOLO...]`, por exemplo:
/// `qm-btc-a:quant_momentum:BTC/USDT;qm-btc-b:quant_momentum:BTC/USDT`.
/// Vazio (o default) produz `Vec::new()` — não um erro.
fn parse_strategy_instances(raw: &str) -> Result<Vec<StrategyInstanceSpec>, ConfigError> {
    raw.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|entry| {
            let invalid = || ConfigError::Invalid {
                name: "STRATEGY_INSTANCES",
                value: entry.to_string(),
                reason:
                    "expected id:kind:SYMBOL[+SYMBOL...], e.g. qm-btc-a:quant_momentum:BTC/USDT"
                        .to_string(),
            };

            let mut parts = entry.splitn(3, ':');
            let id = parts.next().unwrap_or_default().trim();
            let kind = parts.next().unwrap_or_default().trim();
            let symbols_raw = parts.next().ok_or_else(invalid)?.trim();
            if id.is_empty() || kind.is_empty() || symbols_raw.is_empty() {
                return Err(invalid());
            }

            let symbols: Vec<String> = symbols_raw
                .split('+')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            if symbols.is_empty() {
                return Err(invalid());
            }

            Ok(StrategyInstanceSpec {
                id: id.to_string(),
                kind: kind.to_string(),
                symbols,
            })
        })
        .collect()
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            database_url: env_var("DATABASE_URL")?,
            paper_initial_balance: parse_decimal(
                "PAPER_INITIAL_BALANCE",
                &env_var_or("PAPER_INITIAL_BALANCE", "100000"),
            )?,
            paper_maker_fee: parse_decimal(
                "PAPER_MAKER_FEE",
                &env_var_or("PAPER_MAKER_FEE", "0.0005"),
            )?,
            paper_taker_fee: parse_decimal(
                "PAPER_TAKER_FEE",
                &env_var_or("PAPER_TAKER_FEE", "0.001"),
            )?,
            paper_spread_bps: parse_decimal(
                "PAPER_SPREAD_BPS",
                &env_var_or("PAPER_SPREAD_BPS", "2"),
            )?,
            paper_slippage_bps: parse_decimal(
                "PAPER_SLIPPAGE_BPS",
                &env_var_or("PAPER_SLIPPAGE_BPS", "3"),
            )?,
            enabled_symbols: parse_symbols(&env_var_or(
                "ENABLED_SYMBOLS",
                "BTC/USDT,ETH/USDT,SOL/USDT",
            ))?,
            enabled_strategies: parse_list(&env_var_or(
                "ENABLED_STRATEGIES",
                "ema_crossover,momentum,mean_reversion",
            )),
            strategy_instances: parse_strategy_instances(&env_var_or("STRATEGY_INSTANCES", ""))?,
            risk_order_notional: parse_decimal(
                "RISK_ORDER_NOTIONAL",
                &env_var_or("RISK_ORDER_NOTIONAL", "1000"),
            )?,
            risk_max_position_notional: parse_decimal(
                "RISK_MAX_POSITION_NOTIONAL",
                &env_var_or("RISK_MAX_POSITION_NOTIONAL", "2000"),
            )?,
            risk_max_total_exposure: parse_decimal(
                "RISK_MAX_TOTAL_EXPOSURE",
                &env_var_or("RISK_MAX_TOTAL_EXPOSURE", "10000"),
            )?,
            risk_max_open_positions: parse_usize(
                "RISK_MAX_OPEN_POSITIONS",
                &env_var_or("RISK_MAX_OPEN_POSITIONS", "5"),
            )?,
            risk_stop_loss_pct: parse_optional_decimal(
                "RISK_STOP_LOSS_PCT",
                &env_var_or("RISK_STOP_LOSS_PCT", "0.05"),
            )?,
            risk_take_profit_pct: parse_optional_decimal(
                "RISK_TAKE_PROFIT_PCT",
                &env_var_or("RISK_TAKE_PROFIT_PCT", "0.10"),
            )?,
            risk_max_daily_loss: parse_decimal(
                "RISK_MAX_DAILY_LOSS",
                &env_var_or("RISK_MAX_DAILY_LOSS", "2000"),
            )?,
            warmup_candles: parse_u32("WARMUP_CANDLES", &env_var_or("WARMUP_CANDLES", "50"))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_symbol_list() {
        let symbols = parse_symbols("BTC/USDT, ETH/USDT ,SOL/USDT").unwrap();
        assert_eq!(symbols.len(), 3);
        assert_eq!(symbols[0].base, "BTC");
        assert_eq!(symbols[0].quote, "USDT");
        assert_eq!(symbols[2].base, "SOL");
    }

    #[test]
    fn rejects_symbol_without_separator() {
        let result = parse_symbols("BTCUSDT");
        assert!(result.is_err());
    }

    #[test]
    fn parses_comma_separated_strategy_list() {
        let strategies = parse_list("ema_crossover, momentum ,mean_reversion");
        assert_eq!(
            strategies,
            vec!["ema_crossover", "momentum", "mean_reversion"]
        );
    }

    #[test]
    fn empty_optional_decimal_is_none() {
        assert_eq!(
            parse_optional_decimal("RISK_STOP_LOSS_PCT", "").unwrap(),
            None
        );
        assert_eq!(
            parse_optional_decimal("RISK_STOP_LOSS_PCT", "0.05")
                .unwrap()
                .unwrap(),
            Decimal::from_str("0.05").unwrap()
        );
    }

    #[test]
    fn empty_strategy_instances_is_an_empty_list() {
        assert!(parse_strategy_instances("").unwrap().is_empty());
        assert!(parse_strategy_instances("   ").unwrap().is_empty());
    }

    #[test]
    fn parses_strategy_instances_with_multiple_symbols() {
        let instances = parse_strategy_instances(
            "qm-btc-a:quant_momentum:BTC/USDT; ema-multi:ema_crossover:ETH/USDT+SOL/USDT ",
        )
        .unwrap();
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].id, "qm-btc-a");
        assert_eq!(instances[0].kind, "quant_momentum");
        assert_eq!(instances[0].symbols, vec!["BTC/USDT"]);
        assert_eq!(instances[1].id, "ema-multi");
        assert_eq!(instances[1].symbols, vec!["ETH/USDT", "SOL/USDT"]);
    }

    #[test]
    fn rejects_strategy_instance_missing_a_field() {
        assert!(parse_strategy_instances("qm-btc-a:quant_momentum").is_err());
        assert!(parse_strategy_instances(":quant_momentum:BTC/USDT").is_err());
        assert!(parse_strategy_instances("qm-btc-a::BTC/USDT").is_err());
    }

    #[test]
    fn invalid_decimal_produces_named_error() {
        let result = parse_decimal("PAPER_INITIAL_BALANCE", "not-a-number");
        assert!(matches!(
            result,
            Err(ConfigError::Invalid {
                name: "PAPER_INITIAL_BALANCE",
                ..
            })
        ));
    }
}
