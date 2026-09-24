//! Validação read-only de uma série de candles já baixada: ordem temporal,
//! duplicatas e gaps. Nunca corrige nada nem fabrica um candle sintético
//! para preencher um buraco — só relata, para quem chama decidir o que
//! fazer (tipicamente: logar um aviso e seguir, já que um mercado cripto
//! 24/7 pode legitimamente ter gaps reais na exchange).

use domain::{Candle, Timeframe};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GapRange {
    /// `close_time` do último candle antes do gap.
    pub after: chrono::DateTime<chrono::Utc>,
    /// `open_time` do primeiro candle depois do gap.
    pub before: chrono::DateTime<chrono::Utc>,
    /// Quantos candles do timeframe esperado estão faltando neste
    /// intervalo — `(before - after) / timeframe.duration()`.
    pub missing_candles: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ValidationReport {
    /// Índices (na ordem de entrada, não reordenada) onde `open_time` do
    /// candle não é estritamente maior que o do candle anterior mantido.
    pub out_of_order: Vec<usize>,
    /// `open_time`s que aparecem mais de uma vez na série.
    pub duplicates: Vec<chrono::DateTime<chrono::Utc>>,
    pub gaps: Vec<GapRange>,
}

impl ValidationReport {
    pub fn is_clean(&self) -> bool {
        self.out_of_order.is_empty() && self.duplicates.is_empty() && self.gaps.is_empty()
    }

    /// Soma de candles faltando em todos os gaps encontrados — o número
    /// que resume "quanto histórico está faltando", contabilizado nunca
    /// silenciosamente descartado (ver `update::update_symbol` e
    /// `experiments::report`, que sempre reportam isto, mesmo quando é
    /// zero).
    pub fn total_missing_candles(&self) -> i64 {
        self.gaps.iter().map(|g| g.missing_candles).sum()
    }
}

/// Valida `candles` **na ordem em que foram passados** (não ordena por
/// conta própria — ordem temporal é justamente uma das coisas checadas).
/// `timeframe` define o intervalo esperado entre candles consecutivos para
/// a detecção de gaps.
pub fn validate(candles: &[Candle], timeframe: Timeframe) -> ValidationReport {
    let mut report = ValidationReport::default();
    if candles.is_empty() {
        return report;
    }

    let expected_step = timeframe.duration();
    let mut seen_open_times = std::collections::HashSet::new();
    seen_open_times.insert(candles[0].open_time);

    let mut previous = &candles[0];
    for (index, candle) in candles.iter().enumerate().skip(1) {
        if !seen_open_times.insert(candle.open_time) {
            report.duplicates.push(candle.open_time);
            // Uma duplicata não é um "gap" nem uma "desordem" própria —
            // não avança `previous`, para que o próximo candle real seja
            // comparado contra o último legitimamente distinto.
            continue;
        }

        if candle.open_time <= previous.open_time {
            report.out_of_order.push(index);
            continue;
        }

        let gap = candle.open_time - previous.close_time;
        if gap > chrono::Duration::zero() {
            let missing = gap.num_seconds() / expected_step.num_seconds();
            if missing > 0 {
                report.gaps.push(GapRange {
                    after: previous.close_time,
                    before: candle.open_time,
                    missing_candles: missing,
                });
            }
        }

        previous = candle;
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono::Utc;
    use domain::InstrumentId;
    use rust_decimal_macros::dec;

    fn candle_at(open_time: chrono::DateTime<Utc>) -> Candle {
        Candle {
            instrument_id: InstrumentId::new(),
            timeframe: Timeframe::M1,
            open_time,
            close_time: open_time + chrono::Duration::minutes(1),
            open: dec!(100),
            high: dec!(101),
            low: dec!(99),
            close: dec!(100.5),
            volume: dec!(10),
            is_closed: true,
        }
    }

    fn base() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
    }

    #[test]
    fn clean_contiguous_series_has_no_findings() {
        let candles: Vec<Candle> = (0..10)
            .map(|i| candle_at(base() + chrono::Duration::minutes(i)))
            .collect();
        let report = validate(&candles, Timeframe::M1);
        assert!(report.is_clean());
    }

    #[test]
    fn detects_a_gap_and_counts_missing_candles() {
        let candles = vec![
            candle_at(base()),
            candle_at(base() + chrono::Duration::minutes(1)),
            // minutos 2, 3, 4 faltando
            candle_at(base() + chrono::Duration::minutes(5)),
        ];
        let report = validate(&candles, Timeframe::M1);
        assert_eq!(report.gaps.len(), 1);
        assert_eq!(report.gaps[0].missing_candles, 3);
        assert_eq!(report.gaps[0].after, base() + chrono::Duration::minutes(2));
        assert_eq!(report.gaps[0].before, base() + chrono::Duration::minutes(5));
    }

    #[test]
    fn detects_duplicate_open_time() {
        let candles = vec![
            candle_at(base()),
            candle_at(base() + chrono::Duration::minutes(1)),
            candle_at(base() + chrono::Duration::minutes(1)), // duplicata
            candle_at(base() + chrono::Duration::minutes(2)),
        ];
        let report = validate(&candles, Timeframe::M1);
        assert_eq!(
            report.duplicates,
            vec![base() + chrono::Duration::minutes(1)]
        );
        assert!(report.gaps.is_empty(), "a duplicate must not read as a gap");
    }

    #[test]
    fn detects_out_of_order_candle() {
        let candles = vec![
            candle_at(base() + chrono::Duration::minutes(5)),
            candle_at(base()), // fora de ordem
        ];
        let report = validate(&candles, Timeframe::M1);
        assert_eq!(report.out_of_order, vec![1]);
    }
}
