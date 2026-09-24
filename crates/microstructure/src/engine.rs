//! `MicrostructureFeatureEngine`: calcula `MicrostructureSnapshot` a partir
//! do estado do livro reconstruído (`book::LocalOrderBook`) e de uma janela
//! rolante de trades recentes — mesmo papel que `features::engine::FeatureEngine`
//! cumpre para candles, mas para dado de microestrutura. **Não depende do
//! crate `features`** — garantia arquitetural de que os dois nunca se
//! misturam (ver doc do crate raiz).
//!
//! `on_book_update` é pensado para ser chamado depois de cada delta
//! aplicado com sucesso a um `LocalOrderBook` (ver `book::LocalOrderBook::apply_delta`);
//! o melhor bid/ask que o engine usa vem sempre do livro reconstruído via
//! depth diff, nunca do stream `bookTicker` — mantém uma única fonte de
//! verdade para o estado do book. `bookTicker` é persistido (`store.rs`)
//! mas não alimenta o engine nesta fase.

use std::collections::VecDeque;

use chrono::{DateTime, Duration, Utc};
use domain::{BookLevel, InstrumentId, MarketTrade, Side};
use rust_decimal::prelude::ToPrimitive;

use crate::book::{DepthSnapshot, LocalOrderBook};
use crate::config::MicrostructureConfig;
use crate::error::MicrostructureError;
use crate::snapshot::MicrostructureSnapshot;

struct TradeRecord {
    timestamp: DateTime<Utc>,
    side: Side,
    quantity: f64,
}

pub struct MicrostructureFeatureEngine {
    instrument_id: InstrumentId,
    config: MicrostructureConfig,
    trades: VecDeque<TradeRecord>,
    prev_best_bid: Option<BookLevel>,
    prev_best_ask: Option<BookLevel>,
    ofi_events: VecDeque<(DateTime<Utc>, f64)>,
}

impl MicrostructureFeatureEngine {
    pub fn new(instrument_id: InstrumentId, config: MicrostructureConfig) -> Self {
        Self {
            instrument_id,
            config,
            trades: VecDeque::new(),
            prev_best_bid: None,
            prev_best_ask: None,
            ofi_events: VecDeque::new(),
        }
    }

    pub fn config(&self) -> &MicrostructureConfig {
        &self.config
    }

    /// Alimenta um trade executado — só atualiza a janela rolante usada
    /// pelas features de trade tape; não emite um `MicrostructureSnapshot`
    /// (esse papel é de `on_book_update`, chamado sempre que o book muda).
    pub fn on_trade(&mut self, trade: &MarketTrade) {
        let quantity = trade.quantity.to_f64().unwrap_or(0.0);
        self.trades.push_back(TradeRecord {
            timestamp: trade.timestamp,
            side: trade.taker_side,
            quantity,
        });
        self.prune_trades(trade.timestamp);
    }

    fn prune_trades(&mut self, now: DateTime<Utc>) {
        let window = Duration::seconds(self.config.trade_window_secs as i64);
        while let Some(front) = self.trades.front() {
            if now - front.timestamp > window {
                self.trades.pop_front();
            } else {
                break;
            }
        }
    }

    fn prune_ofi(&mut self, now: DateTime<Utc>) {
        let window = Duration::seconds(self.config.ofi_window_secs as i64);
        while let Some(&(ts, _)) = self.ofi_events.front() {
            if now - ts > window {
                self.ofi_events.pop_front();
            } else {
                break;
            }
        }
    }

    /// Recalcula todas as features a partir do estado atual de `book` (deve
    /// já refletir o delta/snapshot mais recente) mais a janela rolante de
    /// trades já acumulada.
    pub fn on_book_update(
        &mut self,
        book: &LocalOrderBook,
        timestamp: DateTime<Utc>,
    ) -> MicrostructureSnapshot {
        self.prune_trades(timestamp);

        let best_bid = book.best_bid();
        let best_ask = book.best_ask();

        if let (Some(prev_bid), Some(prev_ask), Some(bid), Some(ask)) =
            (self.prev_best_bid, self.prev_best_ask, best_bid, best_ask)
        {
            if let Some(increment) = order_flow_increment(prev_bid, prev_ask, bid, ask) {
                self.ofi_events.push_back((timestamp, increment));
            }
        }
        self.prev_best_bid = best_bid;
        self.prev_best_ask = best_ask;
        self.prune_ofi(timestamp);

        let spread_abs = spread(best_bid, best_ask);
        let mid_price = mid(best_bid, best_ask);
        let spread_pct = match (spread_abs, mid_price) {
            (Some(s), Some(m)) if m != 0.0 => Some(s / m),
            _ => None,
        };

        let top_bids = book.top_bids(self.config.depth_levels);
        let top_asks = book.top_asks(self.config.depth_levels);

        let (trade_imbalance, volume_delta, trade_intensity) = self.trade_tape_features();

        MicrostructureSnapshot {
            instrument_id: self.instrument_id,
            timestamp,
            spread_abs,
            spread_pct,
            mid_price,
            microprice: microprice(best_bid, best_ask),
            bid_ask_imbalance: imbalance(
                best_bid.into_iter().collect(),
                best_ask.into_iter().collect(),
            ),
            book_imbalance: imbalance(top_bids, top_asks),
            trade_imbalance,
            volume_delta,
            trade_intensity,
            order_flow_imbalance: ofi_sum(&self.ofi_events),
        }
    }

    fn trade_tape_features(&self) -> (Option<f64>, Option<f64>, Option<f64>) {
        if self.trades.is_empty() {
            return (None, None, None);
        }
        let mut buy_volume = 0.0;
        let mut sell_volume = 0.0;
        for trade in &self.trades {
            match trade.side {
                Side::Buy => buy_volume += trade.quantity,
                Side::Sell => sell_volume += trade.quantity,
            }
        }
        let total = buy_volume + sell_volume;
        let trade_imbalance = if total == 0.0 {
            None
        } else {
            Some((buy_volume - sell_volume) / total)
        };
        let volume_delta = Some(buy_volume - sell_volume);
        let trade_intensity = Some(self.trades.len() as f64 / self.config.trade_window_secs as f64);
        (trade_imbalance, volume_delta, trade_intensity)
    }
}

fn to_f64(level: BookLevel) -> Option<(f64, f64)> {
    Some((level.price.to_f64()?, level.quantity.to_f64()?))
}

fn spread(bid: Option<BookLevel>, ask: Option<BookLevel>) -> Option<f64> {
    let (bp, _) = to_f64(bid?)?;
    let (ap, _) = to_f64(ask?)?;
    Some(ap - bp)
}

fn mid(bid: Option<BookLevel>, ask: Option<BookLevel>) -> Option<f64> {
    let (bp, _) = to_f64(bid?)?;
    let (ap, _) = to_f64(ask?)?;
    Some((bp + ap) / 2.0)
}

/// `(ask.preço·bid.qty + bid.preço·ask.qty) / (bid.qty+ask.qty)` — ver o
/// doc de campo em `snapshot::MicrostructureSnapshot::microprice`.
fn microprice(bid: Option<BookLevel>, ask: Option<BookLevel>) -> Option<f64> {
    let (bp, bq) = to_f64(bid?)?;
    let (ap, aq) = to_f64(ask?)?;
    let denom = bq + aq;
    if denom == 0.0 {
        return None;
    }
    Some((ap * bq + bp * aq) / denom)
}

/// `(soma_qty_bid - soma_qty_ask) / (soma_qty_bid + soma_qty_ask)` sobre os
/// níveis fornecidos — usado tanto para o imbalance L1 (um nível de cada
/// lado) quanto para o imbalance de book (`depth_levels` níveis).
fn imbalance(bids: Vec<BookLevel>, asks: Vec<BookLevel>) -> Option<f64> {
    if bids.is_empty() || asks.is_empty() {
        return None;
    }
    let bid_sum: f64 = bids.iter().filter_map(|l| l.quantity.to_f64()).sum();
    let ask_sum: f64 = asks.iter().filter_map(|l| l.quantity.to_f64()).sum();
    let denom = bid_sum + ask_sum;
    if denom == 0.0 {
        return None;
    }
    Some((bid_sum - ask_sum) / denom)
}

/// Incremento de order-flow imbalance de uma transição de melhor bid/ask
/// para a próxima, fórmula de Cont, Kukanov & Stoikov (2014), "The Price
/// Impact of Order Book Events":
///
/// `e = 1[P^b_t≥P^b_{t-1}]·q^b_t − 1[P^b_t≤P^b_{t-1}]·q^b_{t-1}
///        − (1[P^a_t≤P^a_{t-1}]·q^a_t − 1[P^a_t≥P^a_{t-1}]·q^a_{t-1})`
///
/// Leitura: o bid subir de preço (ou manter preço e aumentar quantidade) é
/// pressão compradora entrando; o bid descer (ou manter preço e reduzir
/// quantidade) é pressão compradora saindo. Simétrico no ask, com o sinal
/// invertido — o ask subir/aumentar quantidade é pressão *vendedora*
/// entrando, o que **reduz** o OFI.
///
/// Exemplo resolvido (o mesmo do teste `order_flow_increment_matches_hand_worked_example`):
/// bid vai de `(100, 5)` para `(100, 8)` (mesmo preço, +3 de quantidade — mais
/// compra entrando) e o ask fica parado em `(101, 5)`. Contribuição do bid =
/// `8 - 5 = 3` (preço igual conta nos dois indicadores). Contribuição do ask =
/// `5 - 5 = 0` (nada mudou). `e = 3 - 0 = 3`: OFI positivo, como esperado de
/// mais pressão compradora sem nenhuma mudança do lado vendedor.
fn order_flow_increment(
    prev_bid: BookLevel,
    prev_ask: BookLevel,
    bid: BookLevel,
    ask: BookLevel,
) -> Option<f64> {
    let (pbp, pbq) = to_f64(prev_bid)?;
    let (pap, paq) = to_f64(prev_ask)?;
    let (bp, bq) = to_f64(bid)?;
    let (ap, aq) = to_f64(ask)?;

    let bid_contribution = (if bp >= pbp { bq } else { 0.0 }) - (if bp <= pbp { pbq } else { 0.0 });
    let ask_contribution = (if ap <= pap { aq } else { 0.0 }) - (if ap >= pap { paq } else { 0.0 });
    Some(bid_contribution - ask_contribution)
}

fn ofi_sum(events: &VecDeque<(DateTime<Utc>, f64)>) -> Option<f64> {
    if events.is_empty() {
        None
    } else {
        Some(events.iter().map(|(_, e)| e).sum())
    }
}

/// Um evento de microestrutura já normalizado, em ordem cronológica — a
/// entrada de `compute_series`, o equivalente em lote de
/// `features::engine::compute_series` para candles.
pub enum MicrostructureEvent {
    Snapshot(DepthSnapshot),
    Delta(domain::OrderBookDelta),
    Trade(MarketTrade),
}

/// Aplica `events` (já ordenados cronologicamente, de um único instrumento)
/// contra um `LocalOrderBook` + `MicrostructureFeatureEngine` novos,
/// devolvendo um `MicrostructureSnapshot` por atualização de book (snapshot
/// ou delta aplicado) — trades só alimentam a janela rolante, não geram uma
/// entrada própria na saída (mesma cadência de `on_book_update`/`on_trade`
/// acima). Propaga o primeiro erro de reconstrução (gap de sequência ou
/// delta antes de qualquer snapshot) em vez de pular silenciosamente —
/// mesma postura de nunca confiar num book potencialmente dessincronizado.
pub fn compute_series(
    instrument_id: InstrumentId,
    events: &[MicrostructureEvent],
    config: MicrostructureConfig,
) -> Result<Vec<MicrostructureSnapshot>, MicrostructureError> {
    let mut book = LocalOrderBook::new(instrument_id);
    let mut engine = MicrostructureFeatureEngine::new(instrument_id, config);
    let mut out = Vec::new();

    for event in events {
        match event {
            MicrostructureEvent::Snapshot(snapshot) => {
                book.seed(snapshot);
                out.push(engine.on_book_update(&book, snapshot.captured_at));
            }
            MicrostructureEvent::Delta(delta) => {
                if book.apply_delta(delta)? {
                    out.push(engine.on_book_update(&book, delta.timestamp));
                }
            }
            MicrostructureEvent::Trade(trade) => engine.on_trade(trade),
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn level(price: rust_decimal::Decimal, quantity: rust_decimal::Decimal) -> BookLevel {
        BookLevel { price, quantity }
    }

    fn trade(
        instrument_id: InstrumentId,
        side: Side,
        qty: rust_decimal::Decimal,
        t: DateTime<Utc>,
    ) -> MarketTrade {
        MarketTrade {
            instrument_id,
            exchange_trade_id: "1".to_string(),
            price: dec!(100),
            quantity: qty,
            taker_side: side,
            timestamp: t,
        }
    }

    fn snapshot(instrument_id: InstrumentId) -> DepthSnapshot {
        DepthSnapshot {
            instrument_id,
            last_update_id: 1,
            bids: vec![level(dec!(100), dec!(4)), level(dec!(99), dec!(10))],
            asks: vec![level(dec!(102), dec!(6)), level(dec!(103), dec!(10))],
            captured_at: Utc::now(),
        }
    }

    #[test]
    fn spread_mid_and_microprice_match_hand_computed_values() {
        let instrument_id = InstrumentId::new();
        let mut book = LocalOrderBook::new(instrument_id);
        book.seed(&snapshot(instrument_id));
        let mut engine =
            MicrostructureFeatureEngine::new(instrument_id, MicrostructureConfig::default());

        let now = Utc::now();
        let out = engine.on_book_update(&book, now);

        // bid=(100,4) ask=(102,6)
        assert_eq!(out.spread_abs, Some(2.0));
        assert_eq!(out.mid_price, Some(101.0));
        assert_eq!(out.spread_pct, Some(2.0 / 101.0));
        // microprice = (102*4 + 100*6) / (4+6) = (408+600)/10 = 100.8
        assert_eq!(out.microprice, Some(100.8));
        // bid_ask_imbalance = (4-6)/(4+6) = -0.2
        assert_eq!(out.bid_ask_imbalance, Some(-0.2));
        // book_imbalance (depth_levels=10, só há 2 níveis de cada lado): bids 4+10=14, asks 6+10=16
        // (14-16)/(14+16) = -2/30
        assert_eq!(out.book_imbalance, Some(-2.0 / 30.0));
    }

    #[test]
    fn trade_tape_features_are_none_before_any_trade() {
        let instrument_id = InstrumentId::new();
        let mut book = LocalOrderBook::new(instrument_id);
        book.seed(&snapshot(instrument_id));
        let mut engine =
            MicrostructureFeatureEngine::new(instrument_id, MicrostructureConfig::default());

        let out = engine.on_book_update(&book, Utc::now());
        assert_eq!(out.trade_imbalance, None);
        assert_eq!(out.volume_delta, None);
        assert_eq!(out.trade_intensity, None);
    }

    #[test]
    fn trade_tape_features_match_hand_computed_values() {
        let instrument_id = InstrumentId::new();
        let mut book = LocalOrderBook::new(instrument_id);
        book.seed(&snapshot(instrument_id));
        let config = MicrostructureConfig {
            trade_window_secs: 10,
            ..MicrostructureConfig::default()
        };
        let mut engine = MicrostructureFeatureEngine::new(instrument_id, config);

        let t0 = Utc::now();
        engine.on_trade(&trade(instrument_id, Side::Buy, dec!(3), t0));
        engine.on_trade(&trade(
            instrument_id,
            Side::Sell,
            dec!(1),
            t0 + Duration::seconds(1),
        ));
        engine.on_trade(&trade(
            instrument_id,
            Side::Buy,
            dec!(2),
            t0 + Duration::seconds(2),
        ));

        let out = engine.on_book_update(&book, t0 + Duration::seconds(2));
        // buy=5, sell=1 -> imbalance=(5-1)/6=4/6; delta=4; intensity=3 trades/10s=0.3
        assert_eq!(out.trade_imbalance, Some(4.0 / 6.0));
        assert_eq!(out.volume_delta, Some(4.0));
        assert_eq!(out.trade_intensity, Some(0.3));
    }

    #[test]
    fn trade_tape_window_evicts_old_trades() {
        let instrument_id = InstrumentId::new();
        let mut book = LocalOrderBook::new(instrument_id);
        book.seed(&snapshot(instrument_id));
        let config = MicrostructureConfig {
            trade_window_secs: 5,
            ..MicrostructureConfig::default()
        };
        let mut engine = MicrostructureFeatureEngine::new(instrument_id, config);

        let t0 = Utc::now();
        engine.on_trade(&trade(instrument_id, Side::Buy, dec!(100), t0));
        // 10s depois, bem fora da janela de 5s -> o trade antigo deve ter sido evictado.
        let out = engine.on_book_update(&book, t0 + Duration::seconds(10));
        assert_eq!(out.trade_imbalance, None);
        assert_eq!(out.volume_delta, None);
        assert_eq!(out.trade_intensity, None);
    }

    #[test]
    fn order_flow_increment_matches_hand_worked_example() {
        // Exemplo do doc comment de `order_flow_increment`: bid (100,5)->(100,8),
        // ask parado em (101,5) -> e = 3.
        let prev_bid = level(dec!(100), dec!(5));
        let prev_ask = level(dec!(101), dec!(5));
        let bid = level(dec!(100), dec!(8));
        let ask = level(dec!(101), dec!(5));
        assert_eq!(
            order_flow_increment(prev_bid, prev_ask, bid, ask),
            Some(3.0)
        );
    }

    #[test]
    fn order_flow_increment_is_negative_when_only_ask_size_grows() {
        // ask (101,5)->(101,8): mais pressão vendedora, bid parado -> e = -3.
        let prev_bid = level(dec!(100), dec!(5));
        let prev_ask = level(dec!(101), dec!(5));
        let bid = level(dec!(100), dec!(5));
        let ask = level(dec!(101), dec!(8));
        assert_eq!(
            order_flow_increment(prev_bid, prev_ask, bid, ask),
            Some(-3.0)
        );
    }

    #[test]
    fn order_flow_imbalance_is_none_until_a_second_book_update_exists() {
        let instrument_id = InstrumentId::new();
        let mut book = LocalOrderBook::new(instrument_id);
        book.seed(&snapshot(instrument_id));
        let mut engine =
            MicrostructureFeatureEngine::new(instrument_id, MicrostructureConfig::default());

        // Primeira leitura: não há "transição" anterior ainda.
        let first = engine.on_book_update(&book, Utc::now());
        assert_eq!(first.order_flow_imbalance, None);
    }

    #[test]
    fn compute_series_reconstructs_book_and_emits_a_snapshot_per_book_update() {
        let instrument_id = InstrumentId::new();
        let snap = snapshot(instrument_id);
        let t0 = snap.captured_at;
        let events = vec![
            MicrostructureEvent::Snapshot(snap),
            MicrostructureEvent::Trade(trade(instrument_id, Side::Buy, dec!(1), t0)),
            MicrostructureEvent::Delta(domain::OrderBookDelta {
                instrument_id,
                first_update_id: 2,
                final_update_id: 2,
                bids: vec![level(dec!(100), dec!(9))],
                asks: vec![],
                timestamp: t0 + Duration::seconds(1),
            }),
        ];

        let out = compute_series(instrument_id, &events, MicrostructureConfig::default()).unwrap();
        // Um snapshot por evento de book (seed + delta) = 2; o trade não conta.
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].bid_ask_imbalance, Some((9.0 - 6.0) / (9.0 + 6.0)));
    }

    #[test]
    fn compute_series_propagates_a_sequence_gap_instead_of_skipping_it() {
        let instrument_id = InstrumentId::new();
        let snap = snapshot(instrument_id);
        let events = vec![
            MicrostructureEvent::Snapshot(snap.clone()),
            MicrostructureEvent::Delta(domain::OrderBookDelta {
                instrument_id,
                first_update_id: 50, // gap: deveria ser 2
                final_update_id: 51,
                bids: vec![],
                asks: vec![],
                timestamp: snap.captured_at + Duration::seconds(1),
            }),
        ];

        let result = compute_series(instrument_id, &events, MicrostructureConfig::default());
        assert!(matches!(
            result,
            Err(MicrostructureError::SequenceGap { .. })
        ));
    }
}
