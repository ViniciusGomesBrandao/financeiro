//! Contexto operacional de um robô (Fase A): unidade isolada de
//! símbolo + timeframe + capital + candidatas.

use domain::{InstrumentId, StrategyId, Timeframe};
use rust_decimal::Decimal;

#[derive(Debug, Clone)]
pub struct RobotContext {
    pub id: String,
    pub instrument_id: InstrumentId,
    pub symbol: String,
    pub timeframe: Timeframe,
    pub strategy_ids: Vec<StrategyId>,
    pub paper_capital: Decimal,
}

impl RobotContext {
    pub fn owns_strategy(&self, id: &StrategyId) -> bool {
        self.strategy_ids.iter().any(|s| s == id)
    }
}

/// Agrupa instrumentos por timeframe para abrir um stream WS por TF.
pub fn group_instruments_by_timeframe(
    robots: &[RobotContext],
    instruments: &[domain::Instrument],
) -> Vec<(Timeframe, Vec<domain::Instrument>)> {
    use std::collections::{HashMap, HashSet};

    let by_id: HashMap<InstrumentId, &domain::Instrument> =
        instruments.iter().map(|i| (i.id, i)).collect();

    let mut groups: HashMap<Timeframe, HashSet<InstrumentId>> = HashMap::new();
    for robot in robots {
        groups
            .entry(robot.timeframe)
            .or_default()
            .insert(robot.instrument_id);
    }

    let mut out = Vec::new();
    for (tf, ids) in groups {
        let list: Vec<_> = ids
            .into_iter()
            .filter_map(|id| by_id.get(&id).map(|i| (*i).clone()))
            .collect();
        if !list.is_empty() {
            out.push((tf, list));
        }
    }
    out.sort_by_key(|(tf, _)| tf.as_str());
    out
}

/// Fan-in dinâmico de streams WS: permite anexar novos receivers em runtime
/// quando um robô hot-load precisa de um símbolo/timeframe ainda não coberto.
pub struct MarketEventHub {
    tx: tokio::sync::mpsc::UnboundedSender<domain::MarketEvent>,
}

impl MarketEventHub {
    pub fn new() -> (
        Self,
        tokio::sync::mpsc::UnboundedReceiver<domain::MarketEvent>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (Self { tx }, rx)
    }

    /// Encaminha todos os eventos de `recv` para o canal compartilhado.
    pub fn attach(
        &self,
        mut recv: tokio::sync::mpsc::UnboundedReceiver<domain::MarketEvent>,
    ) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            while let Some(event) = recv.recv().await {
                if tx.send(event).is_err() {
                    break;
                }
            }
        });
    }
}

/// Mescla vários receivers em um único canal (um stream WS por timeframe).
pub fn merge_market_event_streams(
    receivers: Vec<tokio::sync::mpsc::UnboundedReceiver<domain::MarketEvent>>,
) -> tokio::sync::mpsc::UnboundedReceiver<domain::MarketEvent> {
    let (hub, rx) = MarketEventHub::new();
    for recv in receivers {
        hub.attach(recv);
    }
    // Mantém o hub vivo enquanto os forwards rodarem (clonam o tx).
    std::mem::forget(hub);
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{
        Asset, AssetClass, Exchange, Instrument, InstrumentId, MarketType, StrategyId, Symbol,
        Timeframe,
    };
    use rust_decimal_macros::dec;

    fn instrument(symbol: &str) -> Instrument {
        let (b, q) = symbol.split_once('/').unwrap();
        let base = Asset::new(b).unwrap();
        let quote = Asset::new(q).unwrap();
        Instrument::new(
            Symbol::from_pair(&base, &quote),
            base,
            quote,
            AssetClass::Crypto,
            Exchange::Binance,
            MarketType::Spot,
            dec!(0.01),
            dec!(0.0001),
            dec!(0.0001),
            dec!(10),
        )
    }

    #[test]
    fn groups_robots_by_timeframe_without_duplicating_instruments() {
        let btc = instrument("BTC/USDT");
        let eth = instrument("ETH/USDT");
        let robots = vec![
            RobotContext {
                id: "a".into(),
                instrument_id: btc.id,
                symbol: "BTC/USDT".into(),
                timeframe: Timeframe::M15,
                strategy_ids: vec![StrategyId::new("a::momentum").unwrap()],
                paper_capital: dec!(1000),
            },
            RobotContext {
                id: "b".into(),
                instrument_id: btc.id,
                symbol: "BTC/USDT".into(),
                timeframe: Timeframe::H1,
                strategy_ids: vec![StrategyId::new("b::momentum").unwrap()],
                paper_capital: dec!(1000),
            },
            RobotContext {
                id: "c".into(),
                instrument_id: eth.id,
                symbol: "ETH/USDT".into(),
                timeframe: Timeframe::M15,
                strategy_ids: vec![StrategyId::new("c::momentum").unwrap()],
                paper_capital: dec!(1000),
            },
        ];
        let groups = group_instruments_by_timeframe(&robots, &[btc.clone(), eth.clone()]);
        assert_eq!(groups.len(), 2);
        let m15 = groups.iter().find(|(tf, _)| *tf == Timeframe::M15).unwrap();
        assert_eq!(m15.1.len(), 2);
        let h1 = groups.iter().find(|(tf, _)| *tf == Timeframe::H1).unwrap();
        assert_eq!(h1.1.len(), 1);
        assert_eq!(h1.1[0].id, btc.id);
    }

    #[test]
    fn robot_owns_only_its_strategy_instances() {
        let id = InstrumentId::new();
        let robot = RobotContext {
            id: "btc-15m".into(),
            instrument_id: id,
            symbol: "BTC/USDT".into(),
            timeframe: Timeframe::M15,
            strategy_ids: vec![
                StrategyId::new("btc-15m::momentum").unwrap(),
                StrategyId::new("btc-15m::ema_crossover").unwrap(),
            ],
            paper_capital: dec!(1000),
        };
        assert!(robot.owns_strategy(&StrategyId::new("btc-15m::momentum").unwrap()));
        assert!(!robot.owns_strategy(&StrategyId::new("other::momentum").unwrap()));
    }
}
