//! Amostragem determinística do `MicrostructureFeatureEngine` numa grade
//! de tempo regular (por default, a cada 1s) — o que os eventos brutos
//! (deltas/trades) não são: chegam em intervalos irregulares, na cadência
//! do mercado. Uma grade regular é o que a camada de pesquisa
//! (`analyzer`) precisa para comparar "retorno futuro em N segundos" de
//! forma direta (deslocar N posições no array), sem reamostrar a cada
//! consulta.
//!
//! **Garantia de não-look-ahead**: o snapshot no ponto de grade `t` usa
//! só o estado do book/trade tape construído a partir de eventos com
//! timestamp `<= t` — nunca um evento futuro. Isso é obtido processando
//! os eventos em ordem (via `replay::merge_book_events_and_trades`) e só
//! emitindo uma leitura quando o próximo evento (ou o fim da lista) cruza
//! um ponto de grade, usando o estado acumulado até ali. Testado
//! explicitamente abaixo (`no_look_ahead_...`), no mesmo espírito do
//! teste equivalente em `features::engine`.
//!
//! **Determinismo**: os pontos de grade são alinhados ao epoch UTC (não
//! ao primeiro evento da série), então rodar a mesma janela de dados duas
//! vezes — ou処 dois símbolos diferentes no mesmo intervalo de tempo —
//! produz exatamente os mesmos timestamps de grade, comparáveis entre si —
//! inclusive entre símbolos diferentes na mesma janela de tempo. Quando
//! não há nenhum evento de book entre dois pontos de grade consecutivos
//! (mercado parado, ou um buraco de coleta), o último estado
//! conhecido é repetido (last-observation-carried-forward) — ainda
//! respeitando não-look-ahead, já que é só informação passada relida, não
//! inventada.

use chrono::{DateTime, Duration, Utc};
use domain::InstrumentId;

use crate::book::LocalOrderBook;
use crate::engine::{MicrostructureEvent, MicrostructureFeatureEngine};
use crate::error::MicrostructureError;
use crate::snapshot::MicrostructureSnapshot;
use crate::MicrostructureConfig;

/// Arredonda `ts` para cima até o próximo múltiplo de `interval` a partir
/// do epoch UTC (ou o próprio `ts`, se já estiver exatamente num múltiplo)
/// — a âncora fixa que torna os pontos de grade determinísticos e
/// comparáveis entre execuções/símbolos.
pub fn align_up_to_grid(ts: DateTime<Utc>, interval: Duration) -> DateTime<Utc> {
    let interval_ms = interval.num_milliseconds().max(1);
    let ts_ms = ts.timestamp_millis();
    let floor_ms = ts_ms.div_euclid(interval_ms) * interval_ms;
    let aligned_ms = if floor_ms < ts_ms {
        floor_ms + interval_ms
    } else {
        floor_ms
    };
    DateTime::from_timestamp_millis(aligned_ms).unwrap_or(ts)
}

/// Processa `events` (já ordenados — ver `replay::load_day_events`) e
/// devolve um `MicrostructureSnapshot` por ponto de grade cruzado, do
/// primeiro ponto de grade `>=` o timestamp do primeiro evento até o
/// último ponto de grade `<=` o timestamp do último evento. Propaga o
/// primeiro erro de reconstrução (gap de sequência) em vez de pular
/// silenciosamente, mesma postura de `engine::compute_series`.
pub fn snapshot_grid(
    instrument_id: InstrumentId,
    events: &[MicrostructureEvent],
    config: MicrostructureConfig,
    interval: Duration,
) -> Result<Vec<MicrostructureSnapshot>, MicrostructureError> {
    let mut book = LocalOrderBook::new(instrument_id);
    let mut engine = MicrostructureFeatureEngine::new(instrument_id, config);
    let mut out = Vec::new();
    let mut next_grid: Option<DateTime<Utc>> = None;

    for event in events {
        let event_ts = event_timestamp(event);
        if next_grid.is_none() {
            next_grid = Some(align_up_to_grid(event_ts, interval));
        }

        // Emite todo ponto de grade ESTRITAMENTE anterior ao timestamp
        // deste evento usando o estado ANTES de aplicá-lo — é a garantia
        // central de não-look-ahead: se este `for` aplicasse o evento
        // primeiro e só depois checasse quais pontos de grade ele
        // atravessa (a versão anterior deste código), um evento que
        // muda o book bem no futuro (ex.: depois de um período parado
        // sem eventos) contaminaria retroativamente TODOS os pontos de
        // grade pendentes entre o evento anterior e este, mesmo os que
        // deveriam ter fechado com o estado antigo. Corrigido separando
        // as duas fases: primeiro fecha o passado com o estado antigo,
        // só então aplica o evento novo.
        while let Some(grid_ts) = next_grid {
            if grid_ts >= event_ts {
                break;
            }
            if book.is_seeded() {
                out.push(engine.on_book_update(&book, grid_ts));
            }
            next_grid = Some(grid_ts + interval);
        }

        match event {
            MicrostructureEvent::Snapshot(snapshot) => book.seed(snapshot),
            MicrostructureEvent::Delta(delta) => {
                book.apply_delta(delta)?;
            }
            MicrostructureEvent::Trade(trade) => engine.on_trade(trade),
        }

        // Um ponto de grade que cai exatamente no timestamp deste evento
        // já inclui o efeito dele — "eventos com timestamp <= T" cobre
        // T por definição, não só T-epsilon.
        if let Some(grid_ts) = next_grid {
            if grid_ts == event_ts {
                if book.is_seeded() {
                    out.push(engine.on_book_update(&book, grid_ts));
                }
                next_grid = Some(grid_ts + interval);
            }
        }
    }

    Ok(out)
}

fn event_timestamp(event: &MicrostructureEvent) -> DateTime<Utc> {
    match event {
        MicrostructureEvent::Snapshot(s) => s.captured_at,
        MicrostructureEvent::Delta(d) => d.timestamp,
        MicrostructureEvent::Trade(t) => t.timestamp,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{BookLevel, OrderBookDelta};
    use rust_decimal_macros::dec;

    fn level(price: rust_decimal::Decimal, quantity: rust_decimal::Decimal) -> BookLevel {
        BookLevel { price, quantity }
    }

    fn base_ts() -> DateTime<Utc> {
        // Um instante arbitrário, não alinhado a nenhum múltiplo redondo
        // de segundo — expõe qualquer bug de alinhamento que dependesse
        // por acidente de o primeiro evento já cair numa borda de grade.
        DateTime::from_timestamp_millis(1_700_000_000_347).unwrap()
    }

    #[test]
    fn align_up_rounds_to_the_next_grid_point_from_utc_epoch() {
        let interval = Duration::seconds(1);
        let ts = DateTime::from_timestamp_millis(1_700_000_000_347).unwrap();
        let aligned = align_up_to_grid(ts, interval);
        assert_eq!(aligned.timestamp_millis(), 1_700_000_001_000);
    }

    #[test]
    fn align_up_is_a_no_op_when_already_on_a_grid_point() {
        let interval = Duration::seconds(1);
        let ts = DateTime::from_timestamp_millis(1_700_000_000_000).unwrap();
        assert_eq!(align_up_to_grid(ts, interval), ts);
    }

    #[test]
    fn emits_one_snapshot_per_grid_point_crossed() {
        let instrument_id = InstrumentId::new();
        let t0 = base_ts();
        let snapshot = crate::book::DepthSnapshot {
            instrument_id,
            last_update_id: 1,
            bids: vec![level(dec!(100), dec!(1))],
            asks: vec![level(dec!(101), dec!(1))],
            captured_at: t0,
        };
        // t0 (…347ms) não está alinhado; o primeiro ponto de grade
        // alcançável é …1000ms (+653ms de t0), o segundo …2000ms
        // (+1653ms de t0). Um delta em t0+2500ms alcança esses dois
        // pontos mas não o terceiro (…3000ms, que ficaria a +2653ms de
        // t0, depois do delta) — 2 leituras, não mais.
        let delta = OrderBookDelta {
            instrument_id,
            first_update_id: 2,
            final_update_id: 2,
            bids: vec![level(dec!(100), dec!(2))],
            asks: vec![],
            timestamp: t0 + Duration::milliseconds(2500),
        };

        let events = vec![
            MicrostructureEvent::Snapshot(snapshot),
            MicrostructureEvent::Delta(delta),
        ];
        let out = snapshot_grid(
            instrument_id,
            &events,
            MicrostructureConfig::default(),
            Duration::seconds(1),
        )
        .unwrap();

        assert_eq!(out.len(), 2);
        // Os timestamps de grade devem estar exatamente 1s um do outro.
        for pair in out.windows(2) {
            assert_eq!(
                (pair[1].timestamp - pair[0].timestamp).num_milliseconds(),
                1000
            );
        }
    }

    #[test]
    fn quiet_period_carries_the_last_known_state_forward() {
        let instrument_id = InstrumentId::new();
        let t0 = align_up_to_grid(base_ts(), Duration::seconds(1)) - Duration::milliseconds(1);
        let snapshot = crate::book::DepthSnapshot {
            instrument_id,
            last_update_id: 1,
            bids: vec![level(dec!(100), dec!(1))],
            asks: vec![level(dec!(101), dec!(1))],
            captured_at: t0,
        };
        // Nenhum evento por 5s -> nenhum evento novo, só o snapshot
        // inicial; a grade ainda deve emitir uma leitura por segundo
        // repetindo o mesmo estado do book, até o último evento. t0 fica
        // 1ms antes de um ponto de grade X; o delta chega em X+4999ms,
        // alcançando os pontos X, X+1000, X+2000, X+3000, X+4000 (5
        // pontos — X+5000 fica depois do delta, não alcançado ainda).
        let delta = OrderBookDelta {
            instrument_id,
            first_update_id: 2,
            final_update_id: 2,
            bids: vec![],
            asks: vec![],
            timestamp: t0 + Duration::seconds(5),
        };

        let events = vec![
            MicrostructureEvent::Snapshot(snapshot),
            MicrostructureEvent::Delta(delta),
        ];
        let out = snapshot_grid(
            instrument_id,
            &events,
            MicrostructureConfig::default(),
            Duration::seconds(1),
        )
        .unwrap();

        // Book nunca mudou (o delta não altera níveis), então o mid_price
        // deve ser idêntico em todas as leituras.
        let mids: Vec<_> = out.iter().map(|s| s.mid_price).collect();
        assert!(mids.windows(2).all(|w| w[0] == w[1]));
        assert_eq!(out.len(), 5);
    }

    /// Garantia central do módulo: o snapshot no ponto de grade `t` nunca
    /// muda por causa de um evento que acontece depois de `t` — mesmo
    /// padrão do teste `no_look_ahead_snapshot_at_index_i_is_unaffected_by_future_candles`
    /// em `features::engine`.
    ///
    /// Regressão específica: a primeira versão deste código aplicava a
    /// mutação de cada evento *antes* de checar quais pontos de grade
    /// pendentes ele atravessava — então um evento distante no futuro
    /// (depois de um período parado) "fechava" de uma vez vários pontos
    /// de grade passados já usando o estado *novo*, contaminando
    /// retroativamente leituras que deveriam ter ficado com o estado
    /// antigo. Um teste que só compara `short_run[0]` contra `full_run[0]`
    /// não pega esse bug (o primeiro ponto sempre fecha antes do evento
    /// distante ser processado); é preciso checar os pontos que ficam
    /// pendentes exatamente quando o evento distante chega.
    #[test]
    fn no_look_ahead_grid_point_is_unaffected_by_events_that_happen_later() {
        let instrument_id = InstrumentId::new();
        let t0 = base_ts();
        let snapshot = crate::book::DepthSnapshot {
            instrument_id,
            last_update_id: 1,
            bids: vec![level(dec!(100), dec!(1))],
            asks: vec![level(dec!(101), dec!(1))],
            captured_at: t0,
        };
        // Nenhum evento entre o snapshot e o delta distante -> vários
        // pontos de grade (~2s a ~9s) ficam pendentes até o delta em
        // +10s ser processado. É exatamente o cenário que expôs o bug:
        // um único evento "fechando" várias leituras de uma vez.
        let later_delta = OrderBookDelta {
            instrument_id,
            first_update_id: 2,
            final_update_id: 2,
            bids: vec![level(dec!(150), dec!(99))], // mudaria o melhor bid, se visto cedo demais
            asks: vec![],
            timestamp: t0 + Duration::seconds(10),
        };

        let out = snapshot_grid(
            instrument_id,
            &[
                MicrostructureEvent::Snapshot(snapshot),
                MicrostructureEvent::Delta(later_delta),
            ],
            MicrostructureConfig::default(),
            Duration::seconds(1),
        )
        .unwrap();

        // Vários pontos de grade foram emitidos (não só o primeiro) —
        // sem isso o teste não exercitaria o bug.
        assert!(
            out.len() > 3,
            "esperava vários pontos de grade pendentes antes do delta distante, veio {}",
            out.len()
        );

        // NENHUM ponto emitido antes do timestamp do delta pode refletir
        // o novo melhor bid (150) — todos devem mostrar o book original
        // (melhor bid 100), já que o delta só "acontece" 10s depois do
        // snapshot.
        for snap in &out {
            assert_eq!(
                snap.mid_price,
                Some((100.0 + 101.0) / 2.0),
                "ponto de grade em {} vazou o estado do delta futuro (mid_price mudou)",
                snap.timestamp
            );
        }
    }

    #[test]
    fn empty_events_produce_an_empty_grid() {
        let instrument_id = InstrumentId::new();
        let out = snapshot_grid(
            instrument_id,
            &[],
            MicrostructureConfig::default(),
            Duration::seconds(1),
        )
        .unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn sequence_gap_is_propagated_not_swallowed() {
        let instrument_id = InstrumentId::new();
        let t0 = base_ts();
        let snapshot = crate::book::DepthSnapshot {
            instrument_id,
            last_update_id: 100,
            bids: vec![level(dec!(100), dec!(1))],
            asks: vec![level(dec!(101), dec!(1))],
            captured_at: t0,
        };
        let gapped_delta = OrderBookDelta {
            instrument_id,
            first_update_id: 200, // deveria ser 101
            final_update_id: 201,
            bids: vec![],
            asks: vec![],
            timestamp: t0 + Duration::seconds(1),
        };

        let result = snapshot_grid(
            instrument_id,
            &[
                MicrostructureEvent::Snapshot(snapshot),
                MicrostructureEvent::Delta(gapped_delta),
            ],
            MicrostructureConfig::default(),
            Duration::seconds(1),
        );
        assert!(matches!(
            result,
            Err(MicrostructureError::SequenceGap { .. })
        ));
    }
}
