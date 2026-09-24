//! Orquestra o fluxo incremental completo para um símbolo: lê o que já
//! existe localmente, baixa só o que falta, mescla, valida, regrava o
//! Parquet de 1m e regenera os timeframes derivados — a peça que amarra
//! `download`/`store`/`aggregate`/`validate` num único passo idempotente.

use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};
use domain::{Asset, Instrument, Timeframe};
use market_data::binance::BinanceRestClient;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::error::HistoricalDataError;
use crate::validate::ValidationReport;
use crate::{aggregate, download, store};

/// Âncora de início quando não há nenhum dado local ainda: bem antes de
/// qualquer listing na Binance (o exchange lançou em meados de 2017). A
/// própria API simplesmente começa a responder a partir do primeiro candle
/// real de cada símbolo — não é preciso conhecer a data de listing exata,
/// e o mesmo valor serve para qualquer símbolo.
pub fn earliest_anchor() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2017, 1, 1, 0, 0, 0).unwrap()
}

/// Timeframes derivados que `update_symbol` sempre mantém em sincronia com
/// o 1m local — os três pedidos explicitamente ("derivar 5m, 15m e 1h do
/// 1m").
pub const DERIVED_TIMEFRAMES: [Timeframe; 3] = [Timeframe::M5, Timeframe::M15, Timeframe::H1];

#[derive(Debug)]
pub struct UpdateSummary {
    pub symbol: String,
    pub candles_before: usize,
    pub candles_after: usize,
    pub new_candles: usize,
    pub earliest: Option<DateTime<Utc>>,
    pub latest: Option<DateTime<Utc>>,
    pub validation: ValidationReport,
}

/// O que fica gravado em disco em `validation_1m.json` — o `ValidationReport`
/// sozinho não carrega quando/sobre quantos candles rodou, o que importa
/// para um artefato de auditoria persistente (um log pode rolar e
/// desaparecer; este arquivo é sobrescrito a cada `update_symbol`, sempre,
/// mesmo quando limpo, para que "a validação rodou e não achou nada" seja
/// tão visível quanto "a validação achou gaps").
#[derive(Debug, Serialize, Deserialize)]
pub struct ValidationRecord {
    pub symbol: String,
    pub timeframe: String,
    pub checked_at: DateTime<Utc>,
    pub candle_count: usize,
    pub total_missing_candles: i64,
    pub report: ValidationReport,
}

/// Caminho do relatório de validação persistente de `symbol`/`timeframe`
/// sob `data_dir` — ao lado do Parquet correspondente, não misturado nele.
pub fn validation_report_path(data_dir: &Path, symbol: &str, timeframe: Timeframe) -> PathBuf {
    data_dir
        .join(symbol)
        .join(format!("validation_{timeframe}.json"))
}

/// Atualiza o histórico de 1m de `instrument` sob `data_dir`, baixando só
/// o que falta desde o último candle armazenado (ou desde
/// [`earliest_anchor`] se este é o primeiro download), e regenera os
/// arquivos Parquet de 5m/15m/1h inteiros a partir do 1m resultante.
/// Idempotente: rodar de novo sem novos candles disponíveis é um no-op
/// (além de uma chamada de rede que confirma isso).
pub async fn update_symbol(
    rest: &BinanceRestClient,
    data_dir: &Path,
    instrument: &Instrument,
    symbol: &str,
) -> Result<UpdateSummary, HistoricalDataError> {
    let path_1m = store::candle_file_path(data_dir, symbol, Timeframe::M1);
    let existing = store::read_candles(&path_1m, instrument.id, Timeframe::M1)?.unwrap_or_default();
    let candles_before = existing.len();

    let resume_from = existing
        .last()
        .map(|c| c.close_time)
        .unwrap_or_else(earliest_anchor);

    info!(symbol, since = %resume_from, "fetching new 1m candles");
    let fetched = download::download_range(
        rest,
        instrument.id,
        &instrument.base_asset,
        &instrument.quote_asset,
        Timeframe::M1,
        resume_from,
    )
    .await?;

    let mut merged = existing;
    merged.extend(fetched);
    // Dedup por open_time (o ponto de retomada é o close_time do último
    // candle salvo, que é inclusive no lado da Binance — pode reaparecer
    // na primeira página nova) e reordena defensivamente.
    merged.sort_by_key(|c| c.open_time);
    merged.dedup_by_key(|c| c.open_time);

    let candles_after = merged.len();
    let new_candles = candles_after.saturating_sub(candles_before);

    let validation = crate::validate::validate(&merged, Timeframe::M1);
    let total_missing = validation.total_missing_candles();
    if !validation.is_clean() {
        warn!(
            symbol,
            out_of_order = validation.out_of_order.len(),
            duplicates = validation.duplicates.len(),
            gaps = validation.gaps.len(),
            total_missing_candles = total_missing,
            "validation found issues in the local 1m history (reported, not auto-fixed)"
        );
    }
    // Persistido sempre — limpo ou não — para que o achado de validação
    // seja um artefato durável em disco, nunca só uma linha de log que
    // pode passar despercebida ou desaparecer.
    let record = ValidationRecord {
        symbol: symbol.to_string(),
        timeframe: Timeframe::M1.to_string(),
        checked_at: Utc::now(),
        candle_count: merged.len(),
        total_missing_candles: total_missing,
        report: validation.clone(),
    };
    write_validation_record(
        &validation_report_path(data_dir, symbol, Timeframe::M1),
        &record,
    )?;

    let earliest = merged.first().map(|c| c.open_time);
    let latest = merged.last().map(|c| c.close_time);

    store::write_candles(&path_1m, &merged)?;
    for &timeframe in &DERIVED_TIMEFRAMES {
        let derived = aggregate::resample(&merged, timeframe, instrument.id);
        let path = store::candle_file_path(data_dir, symbol, timeframe);
        store::write_candles(&path, &derived)?;
        info!(symbol, %timeframe, candles = derived.len(), "regenerated derived timeframe");
    }

    Ok(UpdateSummary {
        symbol: symbol.to_string(),
        candles_before,
        candles_after,
        new_candles,
        earliest,
        latest,
        validation,
    })
}

fn write_validation_record(
    path: &Path,
    record: &ValidationRecord,
) -> Result<(), HistoricalDataError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| HistoricalDataError::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let json = serde_json::to_string_pretty(record)
        .expect("ValidationRecord serialization cannot fail (no non-JSON-safe types)");
    std::fs::write(path, json).map_err(|source| HistoricalDataError::Io {
        path: path.display().to_string(),
        source,
    })
}

/// Como `update_symbol`, mas resolve `base`/`quote` para um `Instrument`
/// via `rest.fetch_instrument` primeiro — conveniência para os
/// consumidores (CLI) que só têm o par de ativos, não um `Instrument` já
/// carregado.
pub async fn update_pair(
    rest: &BinanceRestClient,
    data_dir: &Path,
    base: &Asset,
    quote: &Asset,
    symbol: &str,
) -> Result<UpdateSummary, HistoricalDataError> {
    let instrument = rest.fetch_instrument(base, quote).await?;
    update_symbol(rest, data_dir, &instrument, symbol).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate::GapRange;

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "historical-data-validation-test-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn validation_record_is_persisted_even_when_clean() {
        // "Contabilizado e reportado, nunca silenciosamente ignorado"
        // vale também para o caso feliz: um arquivo de validação vazio (0
        // gaps) ainda precisa existir em disco, provando que a validação
        // de fato rodou — não apenas quando há algo errado para relatar.
        let dir = temp_dir();
        let path = validation_report_path(&dir, "BTCUSDT", Timeframe::M1);
        let record = ValidationRecord {
            symbol: "BTCUSDT".to_string(),
            timeframe: Timeframe::M1.to_string(),
            checked_at: Utc::now(),
            candle_count: 100,
            total_missing_candles: 0,
            report: ValidationReport::default(),
        };

        write_validation_record(&path, &record).unwrap();

        assert!(path.exists());
        let read_back: ValidationRecord =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(read_back.candle_count, 100);
        assert_eq!(read_back.total_missing_candles, 0);
        assert!(read_back.report.is_clean());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validation_record_round_trips_gaps_and_missing_count() {
        let dir = temp_dir();
        let path = validation_report_path(&dir, "ETHUSDT", Timeframe::M1);
        let base = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let mut report = ValidationReport::default();
        report.gaps.push(GapRange {
            after: base,
            before: base + chrono::Duration::minutes(4),
            missing_candles: 3,
        });
        let record = ValidationRecord {
            symbol: "ETHUSDT".to_string(),
            timeframe: Timeframe::M1.to_string(),
            checked_at: Utc::now(),
            candle_count: 50,
            total_missing_candles: report.total_missing_candles(),
            report,
        };

        write_validation_record(&path, &record).unwrap();

        let read_back: ValidationRecord =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(read_back.total_missing_candles, 3);
        assert_eq!(read_back.report.gaps.len(), 1);
        assert_eq!(read_back.report.gaps[0].missing_candles, 3);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
