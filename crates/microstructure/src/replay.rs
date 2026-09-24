//! Monta a sequência cronologicamente correta de eventos de microestrutura
//! de um símbolo/dia a partir do que `store` já tem persistido — usado
//! tanto por `bin/replay_microstructure.rs` (reconstrução + resumo de
//! features) quanto por `grid` (amostragem determinística em grade fixa).
//! Extraído para cá porque os dois precisam exatamente da mesma lógica de
//! ordenação — ver `merge_book_events_and_trades` abaixo para o porquê de
//! não bastar ordenar por timestamp de parede.

use std::path::Path;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use domain::MarketTrade;

use crate::engine::MicrostructureEvent;
use crate::error::MicrostructureError;
use crate::store;

fn millis_to_utc(ms: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(ms)
        .single()
        .unwrap_or_else(Utc::now)
}

fn event_timestamp(event: &MicrostructureEvent) -> DateTime<Utc> {
    match event {
        MicrostructureEvent::Snapshot(s) => s.captured_at,
        MicrostructureEvent::Delta(d) => d.timestamp,
        MicrostructureEvent::Trade(t) => t.timestamp,
    }
}

/// Monta a sequência de eventos de um dia na ordem que `compute_series`
/// (e `grid::snapshot_grid`) precisam. **Não** basta ordenar tudo por
/// timestamp de parede: um delta bufferizado pelo coletor *durante* a
/// chamada REST do snapshot inicial (ver `bin/collect_microstructure.rs`)
/// tem timestamp de recebimento local **anterior** ao `captured_at` do
/// snapshot que ele deveria seguir — ordenar ingenuamente por tempo
/// colocaria esse delta antes do snapshot, e a reconstrução falharia com
/// `BookNotSeeded`. A ordem que importa é a de `update_id` (a mesma fonte
/// de verdade que `LocalOrderBook::apply_delta` já usa), não o relógio
/// local; só as trades (sem `update_id`) precisam de timestamp para se
/// posicionar. Por isso: snapshot+delta são ordenados entre si pela
/// própria chave de sequência (`last_update_id`/`final_update_id`), e só
/// depois intercalados com as trades por tempo aproximado.
///
/// Um segundo cuidado: deltas já obsoletos em relação ao snapshot mais
/// antigo do dia (`final_update_id <= last_update_id` do snapshot) são
/// descartados antes da ordenação, em vez de ficarem com uma chave menor
/// que a do snapshot — o mesmo destino que teriam ao vivo (`apply_delta`
/// os ignora silenciosamente via `Ok(false)`), só que decidido aqui em vez
/// de depois do book já estar semeado.
pub fn merge_book_events_and_trades(
    mut book_events: Vec<(i64, MicrostructureEvent)>,
    mut trades: Vec<(DateTime<Utc>, MicrostructureEvent)>,
) -> Vec<MicrostructureEvent> {
    let earliest_snapshot_key = book_events
        .iter()
        .filter(|(_, e)| matches!(e, MicrostructureEvent::Snapshot(_)))
        .map(|(key, _)| *key)
        .min();
    if let Some(earliest_snapshot_key) = earliest_snapshot_key {
        book_events.retain(|(key, event)| {
            !matches!(event, MicrostructureEvent::Delta(_)) || *key > earliest_snapshot_key
        });
    }

    book_events.sort_by_key(|(key, _)| *key);
    trades.sort_by_key(|(ts, _)| *ts);

    let mut out = Vec::with_capacity(book_events.len() + trades.len());
    let mut book_iter = book_events.into_iter().peekable();
    let mut trade_iter = trades.into_iter().peekable();
    loop {
        match (book_iter.peek(), trade_iter.peek()) {
            (Some((_, be)), Some((tt, _))) => {
                if event_timestamp(be) <= *tt {
                    out.push(book_iter.next().unwrap().1);
                } else {
                    out.push(trade_iter.next().unwrap().1);
                }
            }
            (Some(_), None) => out.push(book_iter.next().unwrap().1),
            (None, Some(_)) => out.push(trade_iter.next().unwrap().1),
            (None, None) => break,
        }
    }
    out
}

/// Carrega e ordena todos os eventos persistidos de `symbol` em `date`.
pub fn load_day_events(
    data_dir: &Path,
    symbol: &str,
    date: NaiveDate,
) -> Result<Vec<MicrostructureEvent>, MicrostructureError> {
    let instrument_id = crate::deterministic_instrument_id(symbol);
    let mut book_events: Vec<(i64, MicrostructureEvent)> = Vec::new();

    let snapshot_rows = store::read_book_snapshots_day(data_dir, symbol, date)?;
    let mut by_update_id: std::collections::BTreeMap<i64, Vec<store::BookSnapshotRow>> =
        std::collections::BTreeMap::new();
    for row in snapshot_rows {
        by_update_id
            .entry(row.last_update_id)
            .or_default()
            .push(row);
    }
    for (last_update_id, rows) in by_update_id {
        if let Some(snapshot) = store::rows_to_depth_snapshot(instrument_id, &rows)? {
            book_events.push((last_update_id, MicrostructureEvent::Snapshot(snapshot)));
        }
    }

    let delta_rows = store::read_book_deltas_day(data_dir, symbol, date)?;
    for delta in store::group_delta_rows(instrument_id, &delta_rows)? {
        book_events.push((delta.final_update_id, MicrostructureEvent::Delta(delta)));
    }

    let mut trades: Vec<(DateTime<Utc>, MicrostructureEvent)> = Vec::new();
    let trade_rows = store::read_trades_day(data_dir, symbol, date)?;
    for row in trade_rows {
        let timestamp = millis_to_utc(row.timestamp_ms);
        let trade = MarketTrade {
            instrument_id,
            exchange_trade_id: row.exchange_trade_id,
            price: row.price,
            quantity: row.quantity,
            taker_side: row.taker_side,
            timestamp,
        };
        trades.push((timestamp, MicrostructureEvent::Trade(trade)));
    }

    Ok(merge_book_events_and_trades(book_events, trades))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use domain::{BookLevel, InstrumentId, OrderBookDelta};
    use rust_decimal_macros::dec;

    /// Regressão: um teste real contra a Binance mostrou que o coletor
    /// pode bufferizar um delta *durante* a chamada REST do snapshot
    /// inicial — o delta bufferizado tem timestamp de recebimento local
    /// anterior ao `captured_at` do snapshot que ele deveria seguir.
    /// Ordenar ingenuamente por timestamp de parede colocava esse delta
    /// antes do snapshot e a reconstrução falhava com `BookNotSeeded`.
    #[test]
    fn a_delta_received_before_the_snapshot_it_follows_is_still_ordered_after_it() {
        let instrument_id = InstrumentId::new();
        let snapshot_time = Utc::now();
        let delta_received_before_snapshot = snapshot_time - Duration::milliseconds(500);

        let snapshot = crate::book::DepthSnapshot {
            instrument_id,
            last_update_id: 1000,
            bids: vec![BookLevel {
                price: dec!(100),
                quantity: dec!(1),
            }],
            asks: vec![BookLevel {
                price: dec!(101),
                quantity: dec!(1),
            }],
            captured_at: snapshot_time,
        };
        let delta = OrderBookDelta {
            instrument_id,
            first_update_id: 1001,
            final_update_id: 1002,
            bids: vec![BookLevel {
                price: dec!(100),
                quantity: dec!(5),
            }],
            asks: vec![],
            timestamp: delta_received_before_snapshot,
        };

        let book_events = vec![
            (
                snapshot.last_update_id,
                MicrostructureEvent::Snapshot(snapshot),
            ),
            (delta.final_update_id, MicrostructureEvent::Delta(delta)),
        ];
        let merged = merge_book_events_and_trades(book_events, Vec::new());

        assert_eq!(merged.len(), 2);
        assert!(matches!(merged[0], MicrostructureEvent::Snapshot(_)));
        assert!(matches!(merged[1], MicrostructureEvent::Delta(_)));

        let result = crate::compute_series(
            instrument_id,
            &merged,
            crate::MicrostructureConfig::default(),
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 2);
    }

    #[test]
    fn a_stale_delta_older_than_the_first_snapshot_is_dropped_not_ordered_before_it() {
        let instrument_id = InstrumentId::new();
        let t0 = Utc::now();

        let snapshot = crate::book::DepthSnapshot {
            instrument_id,
            last_update_id: 1000,
            bids: vec![BookLevel {
                price: dec!(100),
                quantity: dec!(1),
            }],
            asks: vec![BookLevel {
                price: dec!(101),
                quantity: dec!(1),
            }],
            captured_at: t0,
        };
        let stale_delta = OrderBookDelta {
            instrument_id,
            first_update_id: 997,
            final_update_id: 998,
            bids: vec![],
            asks: vec![],
            timestamp: t0 - Duration::seconds(1),
        };
        let live_delta = OrderBookDelta {
            instrument_id,
            first_update_id: 1001,
            final_update_id: 1001,
            bids: vec![],
            asks: vec![],
            timestamp: t0 + Duration::seconds(1),
        };

        let book_events = vec![
            (
                snapshot.last_update_id,
                MicrostructureEvent::Snapshot(snapshot),
            ),
            (
                stale_delta.final_update_id,
                MicrostructureEvent::Delta(stale_delta),
            ),
            (
                live_delta.final_update_id,
                MicrostructureEvent::Delta(live_delta),
            ),
        ];
        let merged = merge_book_events_and_trades(book_events, Vec::new());

        assert_eq!(merged.len(), 2, "o delta obsoleto deve ser descartado");
        assert!(matches!(merged[0], MicrostructureEvent::Snapshot(_)));
        assert!(matches!(merged[1], MicrostructureEvent::Delta(_)));
    }

    #[test]
    fn trades_are_interleaved_between_book_events_by_timestamp() {
        let instrument_id = InstrumentId::new();
        let t0 = Utc::now();

        let snapshot = crate::book::DepthSnapshot {
            instrument_id,
            last_update_id: 1,
            bids: vec![BookLevel {
                price: dec!(100),
                quantity: dec!(1),
            }],
            asks: vec![BookLevel {
                price: dec!(101),
                quantity: dec!(1),
            }],
            captured_at: t0,
        };
        let delta = OrderBookDelta {
            instrument_id,
            first_update_id: 2,
            final_update_id: 2,
            bids: vec![],
            asks: vec![],
            timestamp: t0 + Duration::seconds(2),
        };
        let trade = MarketTrade {
            instrument_id,
            exchange_trade_id: "1".to_string(),
            price: dec!(100),
            quantity: dec!(1),
            taker_side: domain::Side::Buy,
            timestamp: t0 + Duration::seconds(1),
        };

        let book_events = vec![
            (
                snapshot.last_update_id,
                MicrostructureEvent::Snapshot(snapshot),
            ),
            (delta.final_update_id, MicrostructureEvent::Delta(delta)),
        ];
        let trades = vec![(trade.timestamp, MicrostructureEvent::Trade(trade))];
        let merged = merge_book_events_and_trades(book_events, trades);

        assert_eq!(merged.len(), 3);
        assert!(matches!(merged[0], MicrostructureEvent::Snapshot(_)));
        assert!(matches!(merged[1], MicrostructureEvent::Trade(_)));
        assert!(matches!(merged[2], MicrostructureEvent::Delta(_)));
    }
}
