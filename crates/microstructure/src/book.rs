//! Reconstrução local do livro de ofertas a partir de um snapshot REST +
//! uma sequência de diffs de WebSocket — o procedimento oficial documentado
//! pela própria Binance para manter um livro local correto no mercado Spot:
//!
//! 1. Bufferizar eventos de diff enquanto busca um snapshot REST
//!    (`GET /api/v3/depth`), que devolve `last_update_id` + níveis atuais.
//! 2. Descartar todo evento com `final_update_id <= last_update_id` (já
//!    coberto pelo snapshot).
//! 3. O primeiro evento a aplicar deve satisfazer
//!    `first_update_id <= last_update_id + 1 <= final_update_id`.
//! 4. Cada evento seguinte precisa encaixar exatamente onde o anterior
//!    parou: a mesma desigualdade acima, agora com `last_update_id` sendo o
//!    `final_update_id` do evento aplicado por último. Isso é uma única
//!    regra aplicada sempre (não dois casos especiais) — ver
//!    [`LocalOrderBook::apply_delta`].
//! 5. Se a regra falhar, o livro está possivelmente dessincronizado: quem
//!    chama deve re-buscar um snapshot novo e semear de novo, nunca
//!    continuar aplicando eventos sobre um estado que pode estar errado.
//!
//! Este módulo nunca corrige/adivinha um gap silenciosamente — ele só
//! relata (`MicrostructureError::SequenceGap`) para quem chama decidir.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use domain::{BookLevel, InstrumentId, OrderBookDelta, OrderBookSnapshot};
use rust_decimal::Decimal;

use crate::error::MicrostructureError;

/// Um snapshot de profundidade completo, como devolvido por
/// `GET /api/v3/depth` — o ponto de partida para semear (ou re-semear) um
/// [`LocalOrderBook`]. Distinto de `domain::OrderBookSnapshot`: este
/// carrega `last_update_id`, necessário para a checagem de continuidade
/// dos diffs subsequentes, que um snapshot de exibição genérico não precisa.
#[derive(Debug, Clone, PartialEq)]
pub struct DepthSnapshot {
    pub instrument_id: InstrumentId,
    pub last_update_id: i64,
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
    pub captured_at: DateTime<Utc>,
}

/// Livro de ofertas reconstruído localmente para um único instrumento.
/// `bids`/`asks` são mantidos em `BTreeMap<preço, quantidade>` — inserção e
/// remoção de nível em O(log n), leitura do topo (ou dos top-N) por
/// iteração ordenada, sem precisar re-ordenar um `Vec` a cada update.
#[derive(Debug, Clone)]
pub struct LocalOrderBook {
    instrument_id: InstrumentId,
    bids: BTreeMap<Decimal, Decimal>,
    asks: BTreeMap<Decimal, Decimal>,
    last_update_id: Option<i64>,
}

impl LocalOrderBook {
    pub fn new(instrument_id: InstrumentId) -> Self {
        Self {
            instrument_id,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            last_update_id: None,
        }
    }

    pub fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    pub fn is_seeded(&self) -> bool {
        self.last_update_id.is_some()
    }

    pub fn last_update_id(&self) -> Option<i64> {
        self.last_update_id
    }

    /// Substitui todo o estado do livro pelo conteúdo de `snapshot`,
    /// descartando qualquer estado anterior — o ponto de partida (ou de
    /// re-sincronização após um gap) da reconstrução.
    pub fn seed(&mut self, snapshot: &DepthSnapshot) {
        self.bids.clear();
        self.asks.clear();
        for level in &snapshot.bids {
            if level.quantity > Decimal::ZERO {
                self.bids.insert(level.price, level.quantity);
            }
        }
        for level in &snapshot.asks {
            if level.quantity > Decimal::ZERO {
                self.asks.insert(level.price, level.quantity);
            }
        }
        self.last_update_id = Some(snapshot.last_update_id);
    }

    /// Aplica um evento de diff. `Ok(true)` = aplicado; `Ok(false)` = evento
    /// obsoleto (já coberto pelo estado atual), ignorado sem mutar nada —
    /// não é um erro, é esperado que o snapshot REST inicial já cubra os
    /// primeiros eventos bufferizados. `Err(SequenceGap)` = o evento não se
    /// encaixa logo após o último aplicado; o livro deve ser considerado
    /// não confiável até `seed` ser chamado de novo com um snapshot fresco.
    pub fn apply_delta(&mut self, delta: &OrderBookDelta) -> Result<bool, MicrostructureError> {
        let last = self
            .last_update_id
            .ok_or(MicrostructureError::BookNotSeeded)?;

        if delta.final_update_id <= last {
            return Ok(false);
        }
        if delta.first_update_id > last + 1 {
            return Err(MicrostructureError::SequenceGap {
                expected: last + 1,
                got: delta.first_update_id,
            });
        }

        for level in &delta.bids {
            apply_level(&mut self.bids, level);
        }
        for level in &delta.asks {
            apply_level(&mut self.asks, level);
        }
        self.last_update_id = Some(delta.final_update_id);
        Ok(true)
    }

    pub fn best_bid(&self) -> Option<BookLevel> {
        self.bids
            .iter()
            .next_back()
            .map(|(&price, &quantity)| BookLevel { price, quantity })
    }

    pub fn best_ask(&self) -> Option<BookLevel> {
        self.asks
            .iter()
            .next()
            .map(|(&price, &quantity)| BookLevel { price, quantity })
    }

    /// Os `n` melhores níveis de bid, do maior preço para o menor.
    pub fn top_bids(&self, n: usize) -> Vec<BookLevel> {
        self.bids
            .iter()
            .rev()
            .take(n)
            .map(|(&price, &quantity)| BookLevel { price, quantity })
            .collect()
    }

    /// Os `n` melhores níveis de ask, do menor preço para o maior.
    pub fn top_asks(&self, n: usize) -> Vec<BookLevel> {
        self.asks
            .iter()
            .take(n)
            .map(|(&price, &quantity)| BookLevel { price, quantity })
            .collect()
    }

    /// Projeta o estado atual como um `domain::OrderBookSnapshot` genérico
    /// (todos os níveis, sem o `last_update_id` interno) — para consumo
    /// fora deste crate, onde a identidade de sequência não importa.
    pub fn snapshot(&self, timestamp: DateTime<Utc>) -> OrderBookSnapshot {
        OrderBookSnapshot {
            instrument_id: self.instrument_id,
            bids: self.top_bids(self.bids.len()),
            asks: self.top_asks(self.asks.len()),
            timestamp,
        }
    }
}

fn apply_level(side: &mut BTreeMap<Decimal, Decimal>, level: &BookLevel) {
    if level.quantity <= Decimal::ZERO {
        side.remove(&level.price);
    } else {
        side.insert(level.price, level.quantity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn level(price: Decimal, quantity: Decimal) -> BookLevel {
        BookLevel { price, quantity }
    }

    fn snapshot(instrument_id: InstrumentId, last_update_id: i64) -> DepthSnapshot {
        DepthSnapshot {
            instrument_id,
            last_update_id,
            bids: vec![
                level(dec!(100), dec!(1)),
                level(dec!(99), dec!(2)),
                level(dec!(98), dec!(3)),
            ],
            asks: vec![
                level(dec!(101), dec!(1)),
                level(dec!(102), dec!(2)),
                level(dec!(103), dec!(3)),
            ],
            captured_at: Utc::now(),
        }
    }

    fn delta(
        instrument_id: InstrumentId,
        first: i64,
        last: i64,
        bids: Vec<BookLevel>,
        asks: Vec<BookLevel>,
    ) -> OrderBookDelta {
        OrderBookDelta {
            instrument_id,
            first_update_id: first,
            final_update_id: last,
            bids,
            asks,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn seeding_populates_best_bid_and_ask() {
        let instrument_id = InstrumentId::new();
        let mut book = LocalOrderBook::new(instrument_id);
        assert!(!book.is_seeded());

        book.seed(&snapshot(instrument_id, 1000));
        assert!(book.is_seeded());
        assert_eq!(book.last_update_id(), Some(1000));
        assert_eq!(book.best_bid(), Some(level(dec!(100), dec!(1))));
        assert_eq!(book.best_ask(), Some(level(dec!(101), dec!(1))));
    }

    #[test]
    fn applying_delta_before_seeding_is_an_error() {
        let instrument_id = InstrumentId::new();
        let mut book = LocalOrderBook::new(instrument_id);
        let result = book.apply_delta(&delta(instrument_id, 1, 1, vec![], vec![]));
        assert!(matches!(result, Err(MicrostructureError::BookNotSeeded)));
    }

    #[test]
    fn stale_delta_is_ignored_without_mutating_state() {
        let instrument_id = InstrumentId::new();
        let mut book = LocalOrderBook::new(instrument_id);
        book.seed(&snapshot(instrument_id, 1000));

        // final_update_id (999) <= last_update_id (1000) -> obsoleto.
        let applied = book
            .apply_delta(&delta(
                instrument_id,
                990,
                999,
                vec![level(dec!(100), dec!(999))], // se fosse aplicado, mudaria o best bid
                vec![],
            ))
            .unwrap();

        assert!(!applied);
        assert_eq!(book.last_update_id(), Some(1000));
        assert_eq!(book.best_bid(), Some(level(dec!(100), dec!(1))));
    }

    #[test]
    fn sequence_gap_is_detected_and_not_applied() {
        let instrument_id = InstrumentId::new();
        let mut book = LocalOrderBook::new(instrument_id);
        book.seed(&snapshot(instrument_id, 1000));

        // first_update_id (1005) > last_update_id+1 (1001) -> faltam eventos no meio.
        let result = book.apply_delta(&delta(
            instrument_id,
            1005,
            1010,
            vec![level(dec!(100), dec!(999))],
            vec![],
        ));

        assert!(matches!(
            result,
            Err(MicrostructureError::SequenceGap {
                expected: 1001,
                got: 1005,
            })
        ));
        // Estado não deve ter mudado.
        assert_eq!(book.last_update_id(), Some(1000));
        assert_eq!(book.best_bid(), Some(level(dec!(100), dec!(1))));
    }

    #[test]
    fn first_event_after_seeding_may_overlap_the_snapshot() {
        let instrument_id = InstrumentId::new();
        let mut book = LocalOrderBook::new(instrument_id);
        book.seed(&snapshot(instrument_id, 1000));

        // U=998 <= last+1(1001) <= u=1002: evento válido mesmo sobrepondo
        // parte do que o snapshot já cobria (comportamento documentado da
        // Binance para o primeiro evento aplicado após um snapshot).
        let applied = book
            .apply_delta(&delta(
                instrument_id,
                998,
                1002,
                vec![level(dec!(100), dec!(5))],
                vec![],
            ))
            .unwrap();

        assert!(applied);
        assert_eq!(book.last_update_id(), Some(1002));
        assert_eq!(book.best_bid(), Some(level(dec!(100), dec!(5))));
    }

    #[test]
    fn zero_quantity_removes_the_level() {
        let instrument_id = InstrumentId::new();
        let mut book = LocalOrderBook::new(instrument_id);
        book.seed(&snapshot(instrument_id, 1000));

        book.apply_delta(&delta(
            instrument_id,
            1001,
            1001,
            vec![level(dec!(100), Decimal::ZERO)],
            vec![],
        ))
        .unwrap();

        // O melhor bid era 100; removido, o novo melhor deve ser 99.
        assert_eq!(book.best_bid(), Some(level(dec!(99), dec!(2))));
    }

    #[test]
    fn new_price_level_is_inserted_in_correct_order() {
        let instrument_id = InstrumentId::new();
        let mut book = LocalOrderBook::new(instrument_id);
        book.seed(&snapshot(instrument_id, 1000));

        // Novo melhor bid, acima do topo atual (100).
        book.apply_delta(&delta(
            instrument_id,
            1001,
            1001,
            vec![level(dec!(100.5), dec!(7))],
            vec![],
        ))
        .unwrap();

        assert_eq!(book.best_bid(), Some(level(dec!(100.5), dec!(7))));
        assert_eq!(
            book.top_bids(4),
            vec![
                level(dec!(100.5), dec!(7)),
                level(dec!(100), dec!(1)),
                level(dec!(99), dec!(2)),
                level(dec!(98), dec!(3)),
            ]
        );
    }

    #[test]
    fn a_sequence_of_deltas_reconstructs_the_expected_final_state() {
        let instrument_id = InstrumentId::new();
        let mut book = LocalOrderBook::new(instrument_id);
        book.seed(&snapshot(instrument_id, 1000));

        // 1) sobe o melhor bid removendo 100, adicionando 100.2
        book.apply_delta(&delta(
            instrument_id,
            1001,
            1001,
            vec![level(dec!(100), Decimal::ZERO), level(dec!(100.2), dec!(4))],
            vec![],
        ))
        .unwrap();
        // 2) reduz a quantidade do melhor ask
        book.apply_delta(&delta(
            instrument_id,
            1002,
            1002,
            vec![],
            vec![level(dec!(101), dec!(0.5))],
        ))
        .unwrap();
        // 3) remove o segundo melhor bid (99)
        book.apply_delta(&delta(
            instrument_id,
            1003,
            1003,
            vec![level(dec!(99), Decimal::ZERO)],
            vec![],
        ))
        .unwrap();

        assert_eq!(book.last_update_id(), Some(1003));
        assert_eq!(book.best_bid(), Some(level(dec!(100.2), dec!(4))));
        assert_eq!(book.best_ask(), Some(level(dec!(101), dec!(0.5))));
        assert_eq!(
            book.top_bids(3),
            vec![level(dec!(100.2), dec!(4)), level(dec!(98), dec!(3))]
        );
    }

    #[test]
    fn snapshot_projection_exposes_all_current_levels() {
        let instrument_id = InstrumentId::new();
        let mut book = LocalOrderBook::new(instrument_id);
        book.seed(&snapshot(instrument_id, 1000));

        let projected = book.snapshot(Utc::now());
        assert_eq!(projected.instrument_id, instrument_id);
        assert_eq!(projected.bids.len(), 3);
        assert_eq!(projected.asks.len(), 3);
        assert_eq!(projected.mid_price(), Some(dec!(100.5)));
    }
}
