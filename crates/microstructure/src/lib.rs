//! Camada de microestrutura do Quant Engine: coleta contínua e
//! reconstrução do livro de ofertas + trade tape da Binance Spot, e um
//! Feature Engine que calcula sinais clássicos de microestrutura (spread,
//! mid, microprice, imbalances, order flow) a partir disso.
//!
//! **Isolamento deliberado**: este crate não depende de `features`
//! (`FeatureSnapshot` OHLCV), `strategies`, `risk`, `portfolio`,
//! `execution`, `persistence` nem `app`. Nenhuma estratégia, tuning ou ML
//! acontece aqui — é só a fundação de dados, pensada para ser consumida
//! por uma fase futura. `MicrostructureSnapshot` (ver `snapshot`) não
//! compartilha nenhum tipo com `features::snapshot::FeatureSnapshot`.
//!
//! Fluxo: `book::LocalOrderBook` reconstrói o estado do livro a partir de
//! um snapshot REST + diffs de WebSocket (`binance::rest`/`binance::ws`),
//! com detecção de gap de sequência (`error::MicrostructureError::SequenceGap`)
//! em vez de aplicar dado potencialmente incorreto silenciosamente.
//! `store` persiste tudo em Parquet (preço/quantidade como `Decimal128`,
//! nunca `f64`), particionado por símbolo+dia. `engine::MicrostructureFeatureEngine`
//! consome o book reconstruído + a trade tape para produzir
//! `snapshot::MicrostructureSnapshot`, tanto incrementalmente (`on_book_update`/`on_trade`,
//! para um coletor ao vivo) quanto em lote (`engine::compute_series`, para
//! reprocessar dado já persistido — ver `bin/replay_microstructure.rs`).

pub mod analyzer;
pub mod binance;
pub mod book;
pub mod compact;
pub mod config;
pub mod desync_log;
pub mod engine;
pub mod error;
pub mod grid;
pub mod replay;
pub mod snapshot;
pub mod store;

pub use book::{DepthSnapshot, LocalOrderBook};
pub use config::MicrostructureConfig;
pub use engine::{compute_series, MicrostructureEvent, MicrostructureFeatureEngine};
pub use error::MicrostructureError;
pub use grid::snapshot_grid;
pub use snapshot::MicrostructureSnapshot;

use uuid::Uuid;

/// Namespace fixo usado para derivar um `InstrumentId` determinístico a
/// partir do símbolo de transmissão (ex. `"BTCUSDT"`) via `Uuid::new_v5`.
/// Mesmo símbolo -> mesmo id sempre, entre reinícios do coletor, sem
/// precisar de rede (REST `exchangeInfo`) nem de banco para resolver
/// identidade — mantém este crate 100% offline, como `historical-data`.
/// Um UUID v4 fixo gerado uma única vez para este projeto, não um valor
/// com significado especial.
const INSTRUMENT_NAMESPACE: Uuid = Uuid::from_bytes([
    0x8f, 0x3a, 0x1c, 0x02, 0x6b, 0x77, 0x4b, 0x1e, 0x9a, 0x5d, 0x2e, 0x0c, 0x4f, 0x81, 0x6a, 0x3b,
]);

/// Deriva o `InstrumentId` determinístico de um símbolo de transmissão —
/// ver `INSTRUMENT_NAMESPACE`.
pub fn deterministic_instrument_id(symbol_wire: &str) -> domain::InstrumentId {
    domain::InstrumentId(Uuid::new_v5(&INSTRUMENT_NAMESPACE, symbol_wire.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_instrument_id_is_stable_across_calls() {
        let a = deterministic_instrument_id("BTCUSDT");
        let b = deterministic_instrument_id("BTCUSDT");
        assert_eq!(a, b);
    }

    #[test]
    fn deterministic_instrument_id_differs_by_symbol() {
        let btc = deterministic_instrument_id("BTCUSDT");
        let eth = deterministic_instrument_id("ETHUSDT");
        assert_ne!(btc, eth);
    }
}
