//! Log de diagnóstico de gap/desincronização — JSONL append-only, **não**
//! Parquet. Gaps devem ser raros por natureza; o custo de indexação
//! columnar do Parquet não se paga para um volume baixo, e um log
//! auditável a olho nu (uma linha JSON por evento) é exatamente o ponto de
//! um log de diagnóstico.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::MicrostructureError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum DesyncKind {
    /// Um evento de diff não se encaixou logo após o último aplicado — ver
    /// `MicrostructureError::SequenceGap`.
    SequenceGap { expected: i64, got: i64 },
    /// Refresh periódico de segurança do snapshot completo, sem que um gap
    /// tenha sido detectado — registrado para auditoria, não é um erro.
    ScheduledResnapshot,
    /// A conexão WebSocket caiu e foi reconectada.
    WsReconnect { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesyncEvent {
    pub symbol: String,
    pub detected_at: DateTime<Utc>,
    pub kind: DesyncKind,
    pub note: String,
}

fn log_path(data_dir: &Path, symbol: &str) -> PathBuf {
    data_dir.join(symbol).join("desync_log.jsonl")
}

pub fn append(data_dir: &Path, event: &DesyncEvent) -> Result<(), MicrostructureError> {
    let path = log_path(data_dir, &event.symbol);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| MicrostructureError::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|source| MicrostructureError::Io {
            path: path.display().to_string(),
            source,
        })?;
    let line = serde_json::to_string(event).map_err(|e| {
        MicrostructureError::Parse(format!("failed to serialize desync event: {e}"))
    })?;
    writeln!(file, "{line}").map_err(|source| MicrostructureError::Io {
        path: path.display().to_string(),
        source,
    })?;
    Ok(())
}

/// Lê todos os eventos já registrados para `symbol` — usado por testes e
/// por ferramentas de auditoria; o coletor em si só grava (`append`).
pub fn read_all(data_dir: &Path, symbol: &str) -> Result<Vec<DesyncEvent>, MicrostructureError> {
    let path = log_path(data_dir, symbol);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&path).map_err(|source| MicrostructureError::Io {
        path: path.display().to_string(),
        source,
    })?;
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .map_err(|e| MicrostructureError::Parse(format!("malformed desync log line: {e}")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "microstructure-desync-test-{label}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn appended_events_round_trip_in_order() {
        let dir = temp_dir("basic");
        let event_a = DesyncEvent {
            symbol: "BTCUSDT".to_string(),
            detected_at: Utc::now(),
            kind: DesyncKind::SequenceGap {
                expected: 101,
                got: 105,
            },
            note: "gap detectado ao aplicar delta".to_string(),
        };
        let event_b = DesyncEvent {
            symbol: "BTCUSDT".to_string(),
            detected_at: Utc::now(),
            kind: DesyncKind::ScheduledResnapshot,
            note: "refresh periódico".to_string(),
        };

        append(&dir, &event_a).unwrap();
        append(&dir, &event_b).unwrap();

        let restored = read_all(&dir, "BTCUSDT").unwrap();
        assert_eq!(restored, vec![event_a, event_b]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reading_a_symbol_with_no_log_yet_returns_empty() {
        let dir = temp_dir("empty");
        let restored = read_all(&dir, "ETHUSDT").unwrap();
        assert!(restored.is_empty());
    }
}
