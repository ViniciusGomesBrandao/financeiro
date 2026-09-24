//! Janelas de período para rodar a mesma configuração (sem nenhum ajuste
//! de parâmetro) sobre o histórico completo e sobre recortes recentes —
//! "os últimos N meses" respondem uma pergunta diferente de "o período
//! inteiro disponível" (estabilidade recente vs. robustez de longo prazo),
//! e nenhuma das duas substitui a outra.

use chrono::Months;
use domain::Candle;

/// As janelas rodadas por padrão: período completo, mais os recortes
/// móveis pedidos (24, 12, 6 e 3 meses a partir do candle mais recente
/// disponível). `None` = sem corte (período completo).
pub const WINDOWS: &[(&str, Option<u32>)] = &[
    ("full", None),
    ("24m", Some(24)),
    ("12m", Some(12)),
    ("6m", Some(6)),
    ("3m", Some(3)),
];

/// Recorta `candles` (deve vir ordenado por `open_time`) para os últimos
/// `months` meses antes do `close_time` do último candle da série — não
/// "agora" no relógio da máquina, para que o recorte seja reprodutível
/// independentemente de quando a ferramenta é rodada. `months = None`
/// devolve `candles` inteiro, sem cópia desnecessária além do clone
/// requerido pela assinatura.
pub fn slice_recent_window(candles: &[Candle], months: Option<u32>) -> Vec<Candle> {
    let Some(months) = months else {
        return candles.to_vec();
    };
    let Some(end) = candles.last().map(|c| c.close_time) else {
        return Vec::new();
    };
    let Some(start) = end.checked_sub_months(Months::new(months)) else {
        // Pedir mais meses do que o calendário suporta subtrair (não deve
        // acontecer na prática) degrada para "período completo" em vez de
        // falhar.
        return candles.to_vec();
    };
    candles
        .iter()
        .filter(|c| c.open_time >= start)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use domain::{InstrumentId, Timeframe};
    use rust_decimal_macros::dec;

    fn candle_at(open_time: chrono::DateTime<Utc>) -> Candle {
        Candle {
            instrument_id: InstrumentId::new(),
            timeframe: Timeframe::D1,
            open_time,
            close_time: open_time + chrono::Duration::days(1),
            open: dec!(100),
            high: dec!(101),
            low: dec!(99),
            close: dec!(100),
            volume: dec!(1),
            is_closed: true,
        }
    }

    #[test]
    fn full_window_returns_everything_unchanged() {
        let candles = vec![
            candle_at(Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap()),
            candle_at(Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()),
        ];
        let sliced = slice_recent_window(&candles, None);
        assert_eq!(sliced.len(), 2);
    }

    #[test]
    fn twelve_month_window_keeps_only_the_last_12_months() {
        // Um candle por mês, de jan/2022 a dez/2024 (36 candles). Uma
        // janela de 12m ancorada no último candle (dez/2024) deve manter
        // só jan/2024 em diante.
        let mut candles = Vec::new();
        for year in 2022..=2024 {
            for month in 1..=12u32 {
                candles.push(candle_at(
                    Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0).unwrap(),
                ));
            }
        }
        assert_eq!(candles.len(), 36);

        let sliced = slice_recent_window(&candles, Some(12));

        assert_eq!(sliced.len(), 12);
        assert_eq!(
            sliced.first().unwrap().open_time,
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        );
        assert_eq!(
            sliced.last().unwrap().open_time,
            Utc.with_ymd_and_hms(2024, 12, 1, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn empty_input_yields_empty_output_for_any_window() {
        assert!(slice_recent_window(&[], Some(3)).is_empty());
        assert!(slice_recent_window(&[], None).is_empty());
    }
}
