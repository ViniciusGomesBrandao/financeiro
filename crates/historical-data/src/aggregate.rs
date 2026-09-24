//! Deriva candles de timeframes maiores (5m/15m/1h/...) a partir de uma
//! série de candles de 1m — nunca o contrário, e nunca busca essas
//! resoluções diretamente da exchange (ver o pedido original: "derivar 5m,
//! 15m e 1h do 1m").
//!
//! **Política de gaps: um bucket só é emitido se estiver completo.** Um
//! candle de 5m/15m/1h só existe na saída se *todos* os candles de 1m que
//! o compõem existirem na entrada — um bucket com qualquer candle de 1m
//! faltando é descartado inteiro, nunca agregado a partir do que sobrou.
//! Isso é deliberado: um OHLCV parcial (ex. high/low computados sobre só 2
//! dos 5 minutos esperados) não é "o candle de 5m verdadeiro", é uma
//! aproximação silenciosamente enviesada que um consumidor não tem como
//! distinguir do candle real — melhor propagar o gap honestamente para o
//! timeframe derivado (como um candle ausente ali também) do que fabricar
//! uma aparência de completude. `validate` continua sendo quem relata esse
//! gap propagado, agora no timeframe derivado também, não só no 1m.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use domain::{Candle, InstrumentId, Timeframe};

/// Reamostra `candles_1m` (não precisa vir ordenado — este módulo ordena
/// defensivamente) em candles de `target`, agrupando por bucket alinhado ao
/// epoch UTC (ex.: buckets de 5m em `:00`, `:05`, `:10`, ...). Um bucket é
/// omitido a menos que tenha exatamente os candles de 1m esperados (ver o
/// doc do módulo) — nunca fabrica um OHLCV sintético para preencher um
/// buraco, e nunca agrega um bucket parcial fingindo que é completo.
pub fn resample(
    candles_1m: &[Candle],
    target: Timeframe,
    instrument_id: InstrumentId,
) -> Vec<Candle> {
    let bucket_seconds = target.duration().num_seconds();
    debug_assert!(bucket_seconds > 0, "target timeframe must be positive");
    debug_assert!(
        bucket_seconds % 60 == 0,
        "target timeframe must be a whole number of 1-minute candles"
    );
    let expected_members = (bucket_seconds / 60) as usize;

    let mut sorted: Vec<&Candle> = candles_1m.iter().collect();
    sorted.sort_by_key(|c| c.open_time);

    // BTreeMap mantém os buckets em ordem de tempo sem precisar de um sort
    // separado no fim — a chave é o índice do bucket (segundos desde o
    // epoch, dividido pela duração do bucket).
    let mut buckets: BTreeMap<i64, Vec<&Candle>> = BTreeMap::new();
    for candle in &sorted {
        let bucket_index = candle.open_time.timestamp() / bucket_seconds;
        buckets.entry(bucket_index).or_default().push(candle);
    }

    buckets
        .into_iter()
        .filter(|(_, members)| members.len() == expected_members)
        .map(|(bucket_index, members)| {
            let bucket_start = bucket_start_from_index(bucket_index, bucket_seconds);
            build_aggregated_candle(instrument_id, target, bucket_start, &members)
        })
        .collect()
}

fn bucket_start_from_index(bucket_index: i64, bucket_seconds: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(bucket_index * bucket_seconds, 0)
        .expect("bucket index derived from a valid in-range timestamp stays in range")
}

fn build_aggregated_candle(
    instrument_id: InstrumentId,
    target: Timeframe,
    bucket_start: DateTime<Utc>,
    members: &[&Candle],
) -> Candle {
    // `members` chega ordenado por `open_time` (herdado de `sorted` acima,
    // preservado pela iteração de inserção no bucket) — por isso o
    // primeiro/último elemento são de fato a abertura/fechamento corretos
    // do bucket, não um índice arbitrário.
    let open = members.first().expect("bucket is never empty").open;
    let close = members.last().expect("bucket is never empty").close;
    let high = members
        .iter()
        .map(|c| c.high)
        .max()
        .expect("bucket is never empty");
    let low = members
        .iter()
        .map(|c| c.low)
        .min()
        .expect("bucket is never empty");
    let volume = members.iter().map(|c| c.volume).sum();

    Candle {
        instrument_id,
        timeframe: target,
        open_time: bucket_start,
        close_time: bucket_start + target.duration(),
        open,
        high,
        low,
        close,
        volume,
        is_closed: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rust_decimal_macros::dec;

    fn minute_candle(
        base: DateTime<Utc>,
        minute_offset: i64,
        close: rust_decimal::Decimal,
    ) -> Candle {
        let open_time = base + chrono::Duration::minutes(minute_offset);
        Candle {
            instrument_id: InstrumentId::new(),
            timeframe: Timeframe::M1,
            open_time,
            close_time: open_time + chrono::Duration::minutes(1),
            open: close,
            high: close + dec!(1),
            low: close - dec!(1),
            close,
            volume: dec!(10),
            is_closed: true,
        }
    }

    #[test]
    fn aggregates_a_full_clean_bucket() {
        // Base já alinhada a um múltiplo de 5 minutos, para isolar a
        // agregação em si da lógica de alinhamento de boundary (testada
        // separadamente abaixo).
        let base = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let instrument_id = InstrumentId::new();
        let candles_1m: Vec<Candle> = (0..5)
            .map(|i| minute_candle(base, i, dec!(100) + rust_decimal::Decimal::from(i)))
            .collect();

        let candles_5m = resample(&candles_1m, Timeframe::M5, instrument_id);

        assert_eq!(candles_5m.len(), 1);
        let bucket = &candles_5m[0];
        assert_eq!(bucket.open_time, base);
        assert_eq!(bucket.close_time, base + chrono::Duration::minutes(5));
        assert_eq!(bucket.open, dec!(100)); // primeiro candle
        assert_eq!(bucket.close, dec!(104)); // último candle
        assert_eq!(bucket.high, dec!(105)); // max(close)+1 do último candle (104+1)
        assert_eq!(bucket.low, dec!(99)); // min(close)-1 do primeiro candle (100-1)
        assert_eq!(bucket.volume, dec!(50)); // 5 candles * volume 10
        assert!(bucket.is_closed);
    }

    #[test]
    fn a_bucket_missing_any_1m_candle_is_dropped_entirely_never_partially_aggregated() {
        // Bucket de 5m com só 2 dos 5 candles de 1m esperados (um gap real
        // no meio) — a política agora é descartar o bucket inteiro, nunca
        // agregar a partir do que sobrou (isso pareceria um candle de 5m
        // completo e legítimo, mascarando o gap real).
        let base = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let instrument_id = InstrumentId::new();
        let candles_1m = vec![
            minute_candle(base, 0, dec!(100)),
            // minutos 1, 2, 3 faltando (gap)
            minute_candle(base, 4, dec!(110)),
        ];

        let candles_5m = resample(&candles_1m, Timeframe::M5, instrument_id);

        assert!(
            candles_5m.is_empty(),
            "an incomplete bucket must never be emitted, partially aggregated or otherwise"
        );
    }

    #[test]
    fn boundary_alignment_uses_utc_epoch_not_the_first_candle_in_the_array() {
        // Um bucket incompleto na frente do array (minutos :02,:03 — só 2
        // dos 5 esperados de [:00,:05)) é descartado; o bucket completo
        // que vem depois (minutos :05..:09, os 5 de [:05,:10)) precisa ter
        // seu open_time calculado a partir do epoch UTC (:05), não do
        // primeiro candle do array inteiro (que está em :02) — prova que o
        // alinhamento de boundary não depende da ordem/posição de entrada,
        // mesmo com a política de descartar buckets incompletos.
        let base = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let instrument_id = InstrumentId::new();
        let mut candles_1m = vec![
            minute_candle(base, 2, dec!(100)), // :02, bucket [:00,:05) incompleto
            minute_candle(base, 3, dec!(101)), // :03, idem
        ];
        candles_1m.extend((5..10).map(|i| minute_candle(base, i, dec!(200)))); // :05..:09, completo

        let candles_5m = resample(&candles_1m, Timeframe::M5, instrument_id);

        assert_eq!(
            candles_5m.len(),
            1,
            "só o bucket completo [:05,:10) deve sobreviver"
        );
        assert_eq!(candles_5m[0].open_time, base + chrono::Duration::minutes(5));
    }

    #[test]
    fn empty_input_produces_no_buckets() {
        let candles_5m = resample(&[], Timeframe::M5, InstrumentId::new());
        assert!(candles_5m.is_empty());
    }

    #[test]
    fn two_consecutive_buckets_are_kept_separate_and_ordered() {
        let base = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let instrument_id = InstrumentId::new();
        let mut candles_1m: Vec<Candle> =
            (0..5).map(|i| minute_candle(base, i, dec!(100))).collect();
        candles_1m.extend((5..10).map(|i| minute_candle(base, i, dec!(200))));

        let candles_5m = resample(&candles_1m, Timeframe::M5, instrument_id);

        assert_eq!(candles_5m.len(), 2);
        assert!(candles_5m[0].open_time < candles_5m[1].open_time);
        assert_eq!(candles_5m[1].open_time, base + chrono::Duration::minutes(5));
    }
}
