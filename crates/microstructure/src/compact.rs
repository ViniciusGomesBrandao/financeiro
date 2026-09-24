//! Compactação de partições Parquet: junta os vários arquivos imutáveis
//! que `store`/o coletor foram gravando a cada flush de um (símbolo,
//! tipo, dia) num único arquivo maior. Útil depois de um período com
//! muitos flushes pequenos (reconexões, um `flush_interval_secs` curto
//! demais) — reduz o overhead de metadados/footer por arquivo do Parquet
//! e deixa o diretório mais rápido de listar/ler.
//!
//! **Segurança**: o arquivo novo (`store::COMPACTED_FILE_NAME`) só
//! substitui os originais depois de já estar gravado e promovido para o
//! nome final — nenhum arquivo original é apagado antes disso. Se a etapa
//! de limpeza dos originais falhar no meio (processo morto, disco cheio),
//! `read_all_batches` (`store.rs`) já ignora qualquer arquivo que não seja
//! o compactado assim que ele existe, então um resto de arquivo antigo
//! nunca é lido de novo — nunca há contagem duplicada, só lixo cosmético
//! até uma limpeza manual ou uma nova chamada de compactação (idempotente:
//! uma partição já compactada é detectada e pulada).
//!
//! **Não rode contra o dia de hoje enquanto o coletor estiver ativo**: a
//! compactação lê tudo o que existe no momento e depois apaga os
//! originais — rodar isso ao mesmo tempo que o coletor ainda está fazendo
//! flush no mesmo diretório pode fazer a leitura perder um flush que
//! aconteceu no meio, ou apagar um arquivo que o coletor acabou de criar.
//! É uma ferramenta offline, pensada para dias já encerrados.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{NaiveDate, Utc};

use crate::error::MicrostructureError;
use crate::store::{self, COMPACTED_FILE_NAME};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionKind {
    Trades,
    BookTicker,
    BookDeltas,
    BookSnapshots,
}

impl PartitionKind {
    pub fn label(&self) -> &'static str {
        match self {
            PartitionKind::Trades => "trades",
            PartitionKind::BookTicker => "book_ticker",
            PartitionKind::BookDeltas => "book_deltas",
            PartitionKind::BookSnapshots => "book_snapshots",
        }
    }

    pub const ALL: [PartitionKind; 4] = [
        PartitionKind::Trades,
        PartitionKind::BookTicker,
        PartitionKind::BookDeltas,
        PartitionKind::BookSnapshots,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactSummary {
    pub kind: &'static str,
    /// `None` quando o diretório da partição nem existe (nada a fazer).
    pub files_before: usize,
    pub files_after: usize,
    pub rows: usize,
    pub skipped_already_compact: bool,
}

fn list_parquet_files(dir: &Path) -> Result<Vec<PathBuf>, MicrostructureError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    Ok(fs::read_dir(dir)
        .map_err(|source| MicrostructureError::Io {
            path: dir.display().to_string(),
            source,
        })?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "parquet"))
        .collect())
}

/// Compacta uma única partição (símbolo+tipo+dia). Pula (sem erro) se já
/// não há o que fazer: diretório inexistente, vazio, ou já reduzido a um
/// único `compacted.parquet`.
pub fn compact_partition(
    data_dir: &Path,
    symbol: &str,
    kind: PartitionKind,
    date: NaiveDate,
) -> Result<CompactSummary, MicrostructureError> {
    let dir = store::day_dir(data_dir, symbol, kind.label(), date);
    let original_files = list_parquet_files(&dir)?;
    let files_before = original_files.len();

    let already_compact = files_before == 0
        || (files_before == 1
            && original_files[0].file_name().and_then(|n| n.to_str()) == Some(COMPACTED_FILE_NAME));
    if already_compact {
        return Ok(CompactSummary {
            kind: kind.label(),
            files_before,
            files_after: files_before,
            rows: 0,
            skipped_already_compact: true,
        });
    }

    let tmp_dir = dir.join(".compacting_tmp");
    fs::create_dir_all(&tmp_dir).map_err(|source| MicrostructureError::Io {
        path: tmp_dir.display().to_string(),
        source,
    })?;

    let now = Utc::now();
    let rows = match kind {
        PartitionKind::Trades => {
            let rows = store::read_trades_day(data_dir, symbol, date)?;
            store::write_trades(&tmp_dir, now, &rows)?;
            rows.len()
        }
        PartitionKind::BookTicker => {
            let rows = store::read_book_ticker_day(data_dir, symbol, date)?;
            store::write_book_ticker(&tmp_dir, now, &rows)?;
            rows.len()
        }
        PartitionKind::BookDeltas => {
            let rows = store::read_book_deltas_day(data_dir, symbol, date)?;
            store::write_book_deltas(&tmp_dir, now, &rows)?;
            rows.len()
        }
        PartitionKind::BookSnapshots => {
            let rows = store::read_book_snapshots_day(data_dir, symbol, date)?;
            store::write_book_snapshot(&tmp_dir, now, &rows)?;
            rows.len()
        }
    };

    let written = list_parquet_files(&tmp_dir)?;
    let written_file = written
        .into_iter()
        .next()
        .ok_or_else(|| MicrostructureError::Io {
            path: tmp_dir.display().to_string(),
            source: std::io::Error::other("compaction write produced no output file"),
        })?;

    let final_path = dir.join(COMPACTED_FILE_NAME);
    // Promove o arquivo novo para o nome final ANTES de tocar nos
    // originais — se esta etapa falhar, os originais continuam intactos e
    // nada foi perdido.
    fs::rename(&written_file, &final_path).map_err(|source| MicrostructureError::Io {
        path: final_path.display().to_string(),
        source,
    })?;
    let _ = fs::remove_dir_all(&tmp_dir);

    // Só agora remove os arquivos originais — o novo já está seguro no
    // lugar. Uma falha parcial aqui deixa arquivos "lixo" que
    // `read_all_batches` já ignora (ver o doc de `COMPACTED_FILE_NAME`),
    // nunca uma leitura incorreta.
    for path in &original_files {
        if path != &final_path {
            let _ = fs::remove_file(path);
        }
    }

    let files_after = list_parquet_files(&dir)?.len();
    Ok(CompactSummary {
        kind: kind.label(),
        files_before,
        files_after,
        rows,
        skipped_already_compact: false,
    })
}

/// Compacta as quatro partições de um símbolo/dia.
pub fn compact_day(
    data_dir: &Path,
    symbol: &str,
    date: NaiveDate,
) -> Result<Vec<CompactSummary>, MicrostructureError> {
    PartitionKind::ALL
        .iter()
        .map(|&kind| compact_partition(data_dir, symbol, kind, date))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::TradeRow;
    use domain::Side;
    use rust_decimal_macros::dec;

    fn temp_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "microstructure-compact-test-{label}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn compacts_many_small_flush_files_into_one_preserving_all_rows() {
        let base = temp_dir("basic");
        let symbol = "TESTUSDT";
        let date = Utc::now().date_naive();
        let dir = store::day_dir(&base, symbol, "trades", date);

        for i in 0..5 {
            let row = TradeRow {
                exchange_trade_id: i.to_string(),
                price: dec!(100),
                quantity: dec!(1),
                taker_side: Side::Buy,
                timestamp_ms: i,
            };
            store::write_trades(&dir, Utc::now(), std::slice::from_ref(&row)).unwrap();
        }
        assert_eq!(list_parquet_files(&dir).unwrap().len(), 5);

        let summary = compact_partition(&base, symbol, PartitionKind::Trades, date).unwrap();
        assert_eq!(summary.files_before, 5);
        assert_eq!(summary.files_after, 1);
        assert_eq!(summary.rows, 5);
        assert!(!summary.skipped_already_compact);

        let restored = store::read_trades_day(&base, symbol, date).unwrap();
        assert_eq!(restored.len(), 5);
        let mut ids: Vec<i64> = restored
            .iter()
            .map(|r| r.exchange_trade_id.parse().unwrap())
            .collect();
        ids.sort();
        assert_eq!(ids, vec![0, 1, 2, 3, 4]);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn already_compact_partition_is_skipped_without_touching_files() {
        let base = temp_dir("skip");
        let symbol = "TESTUSDT";
        let date = Utc::now().date_naive();
        let dir = store::day_dir(&base, symbol, "trades", date);

        let row = TradeRow {
            exchange_trade_id: "1".to_string(),
            price: dec!(100),
            quantity: dec!(1),
            taker_side: Side::Buy,
            timestamp_ms: 0,
        };
        store::write_trades(&dir, Utc::now(), std::slice::from_ref(&row)).unwrap();
        let first = compact_partition(&base, symbol, PartitionKind::Trades, date).unwrap();
        assert!(!first.skipped_already_compact);

        let second = compact_partition(&base, symbol, PartitionKind::Trades, date).unwrap();
        assert!(second.skipped_already_compact);
        assert_eq!(second.files_before, 1);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn missing_partition_is_skipped_without_error() {
        let base = temp_dir("missing");
        let symbol = "NOPE";
        let date = Utc::now().date_naive();
        let summary = compact_partition(&base, symbol, PartitionKind::Trades, date).unwrap();
        assert!(summary.skipped_already_compact);
        assert_eq!(summary.files_before, 0);
    }

    /// Regressão da garantia de segurança central deste módulo: se um
    /// arquivo original "sobrevive" por qualquer motivo depois da
    /// compactação (aqui, simulado escrevendo um arquivo extra depois),
    /// a leitura ainda não duplica nada — `compacted.parquet` domina.
    #[test]
    fn stray_leftover_file_after_compaction_never_causes_double_counting() {
        let base = temp_dir("stray");
        let symbol = "TESTUSDT";
        let date = Utc::now().date_naive();
        let dir = store::day_dir(&base, symbol, "trades", date);

        let row = TradeRow {
            exchange_trade_id: "1".to_string(),
            price: dec!(100),
            quantity: dec!(1),
            taker_side: Side::Buy,
            timestamp_ms: 0,
        };
        store::write_trades(&dir, Utc::now(), std::slice::from_ref(&row)).unwrap();
        compact_partition(&base, symbol, PartitionKind::Trades, date).unwrap();

        // Simula uma limpeza que não terminou: um arquivo de flush antigo
        // (com as mesmas linhas) reaparece ao lado do compactado.
        store::write_trades(&dir, Utc::now(), std::slice::from_ref(&row)).unwrap();

        let restored = store::read_trades_day(&base, symbol, date).unwrap();
        assert_eq!(
            restored.len(),
            1,
            "compacted.parquet deve ser a única fonte lida, ignorando o arquivo solto"
        );

        let _ = fs::remove_dir_all(&base);
    }
}
