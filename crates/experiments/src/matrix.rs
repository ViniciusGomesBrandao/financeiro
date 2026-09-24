//! Configuração fixa de risco/paper trading usada por todo experimento —
//! copiada literalmente dos defaults de produção em `AppConfig::from_env`
//! (`crates/app/src/config.rs`, os mesmos valores documentados em
//! `.env.example`), não inventada para esta fase. "Usando os parâmetros
//! default atuais" cobre tanto os parâmetros de cada estratégia
//! (`Xxx::default()`, ver `strategy_set`) quanto estes.

use std::str::FromStr;

use domain::Money;
use execution::PaperBrokerConfig;
use risk::RiskConfig;
use rust_decimal::Decimal;

fn d(literal: &str) -> Decimal {
    Decimal::from_str(literal).expect("hardcoded default literal must parse")
}

pub struct DefaultConfig {
    pub initial_balance: Money,
    pub risk: RiskConfig,
    pub broker: PaperBrokerConfig,
}

pub fn default_config() -> DefaultConfig {
    DefaultConfig {
        initial_balance: Money::new(d("100000")),
        risk: RiskConfig {
            order_notional: Money::new(d("1000")),
            max_position_notional: Money::new(d("2000")),
            max_total_exposure: Money::new(d("10000")),
            max_open_positions: 5,
            stop_loss_pct: Some(d("0.05")),
            take_profit_pct: Some(d("0.10")),
            max_daily_loss: Money::new(d("2000")),
        },
        broker: PaperBrokerConfig {
            maker_fee: d("0.0005"),
            taker_fee: d("0.001"),
            spread_bps: d("2"),
            slippage_bps: d("3"),
        },
    }
}
