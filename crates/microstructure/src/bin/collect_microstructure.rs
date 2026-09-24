//! Coletor de longa duração: para cada símbolo configurado, semeia o
//! `LocalOrderBook` via snapshot REST, abre o combined stream
//! (depth/bookTicker/trade) e persiste tudo em Parquet
//! (`microstructure::store`), re-sincronizando o book sempre que um gap de
//! sequência é detectado ou a conexão cai e volta. Puro I/O — não roda o
//! `MicrostructureFeatureEngine` (ver `bin/replay_microstructure.rs` e o
//! doc do crate raiz para a separação deliberada).

use std::path::{Path, PathBuf};
use std::time::Duration as StdDuration;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Parser;
use domain::{BookLevel, BookTicker, InstrumentId, MarketTrade, OrderBookDelta};
use microstructure::binance::{
    wire_symbol, BinanceDepthClient, BinanceMicrostructureWsClient, MicrostructureWsEvent,
};
use microstructure::book::{DepthSnapshot, LocalOrderBook};
use microstructure::desync_log::{self, DesyncEvent, DesyncKind};
use microstructure::error::MicrostructureError;
use microstructure::store::{self, BookDeltaRow, BookSnapshotRow, BookTickerRow, TradeRow};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
struct Args {
    /// Pares base/quote separados por vírgula.
    #[arg(long, default_value = "BTC/USDT,ETH/USDT,SOL/USDT")]
    symbols: String,
    /// Diretório onde os arquivos Parquet são gravados.
    #[arg(long, default_value = "data/microstructure")]
    data_dir: PathBuf,
    /// Intervalo entre flushes do buffer para Parquet — o principal
    /// controle de quantos arquivos são criados por dia. 300s (5min) é o
    /// default: em teste real contra BTC/ETH/SOL, `book_deltas` sozinho
    /// gera centenas de linhas por segundo em mercado líquido — um
    /// intervalo curto (o antigo default de 60s, ou o de 20s usado num
    /// teste manual) produz um arquivo minúsculo atrás do outro. 5min
    /// ainda limita a perda em caso de crash a uma janela pequena, e gera
    /// uma ordem de grandeza menos arquivos por dia.
    #[arg(long, default_value_t = 300)]
    flush_interval_secs: u64,
    /// Tamanho de buffer (linhas, somando trades+ticker+deltas) que força
    /// um flush antecipado mesmo antes do intervalo de tempo — rede de
    /// segurança contra um buffer crescendo sem limite numa rajada
    /// (ex.: uma tempestade de reconexões), não o gatilho normal de
    /// flush. 500_000 é generoso o bastante para nunca disparar em
    /// operação normal (ver o comentário de `flush_interval_secs`), só em
    /// cenários patológicos.
    #[arg(long, default_value_t = 500_000)]
    flush_max_rows: usize,
    /// Intervalo entre refreshes completos de snapshot, como rede de
    /// segurança extra contra desvio silencioso.
    #[arg(long, default_value_t = 1800)]
    resnapshot_interval_secs: u64,
    /// Profundidade do snapshot REST inicial (máximo aceito pela Binance).
    #[arg(long, default_value_t = 5000)]
    depth_limit: u32,
}

fn parse_pairs(raw: &str) -> Result<Vec<(String, String)>> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|pair| {
            let (base, quote) = pair
                .split_once('/')
                .with_context(|| format!("expected BASE/QUOTE, got {pair:?}"))?;
            Ok((base.to_string(), quote.to_string()))
        })
        .collect()
}

fn depth_snapshot_to_rows(snapshot: &DepthSnapshot, captured_at_ms: i64) -> Vec<BookSnapshotRow> {
    let mut rows = Vec::with_capacity(snapshot.bids.len() + snapshot.asks.len());
    for (idx, level) in snapshot.bids.iter().enumerate() {
        rows.push(BookSnapshotRow {
            last_update_id: snapshot.last_update_id,
            is_bid: true,
            level_index: idx as i32,
            price: level.price,
            quantity: level.quantity,
            captured_at_ms,
        });
    }
    for (idx, level) in snapshot.asks.iter().enumerate() {
        rows.push(BookSnapshotRow {
            last_update_id: snapshot.last_update_id,
            is_bid: false,
            level_index: idx as i32,
            price: level.price,
            quantity: level.quantity,
            captured_at_ms,
        });
    }
    rows
}

fn delta_to_rows(delta: &OrderBookDelta) -> Vec<BookDeltaRow> {
    let event_time_ms = delta.timestamp.timestamp_millis();
    let mut rows = Vec::with_capacity(delta.bids.len() + delta.asks.len());
    let push = |rows: &mut Vec<BookDeltaRow>, level: &BookLevel, is_bid: bool| {
        rows.push(BookDeltaRow {
            first_update_id: delta.first_update_id,
            final_update_id: delta.final_update_id,
            is_bid,
            price: level.price,
            quantity: level.quantity,
            event_time_ms,
        });
    };
    for level in &delta.bids {
        push(&mut rows, level, true);
    }
    for level in &delta.asks {
        push(&mut rows, level, false);
    }
    rows
}

fn ticker_to_row(ticker: &BookTicker) -> BookTickerRow {
    BookTickerRow {
        update_id: ticker.update_id,
        bid_price: ticker.bid_price,
        bid_qty: ticker.bid_qty,
        ask_price: ticker.ask_price,
        ask_qty: ticker.ask_qty,
        timestamp_ms: ticker.timestamp.timestamp_millis(),
    }
}

fn trade_to_row(trade: &MarketTrade) -> TradeRow {
    TradeRow {
        exchange_trade_id: trade.exchange_trade_id.clone(),
        price: trade.price,
        quantity: trade.quantity,
        taker_side: trade.taker_side,
        timestamp_ms: trade.timestamp.timestamp_millis(),
    }
}

/// Busca um snapshot novo, persiste, e re-semeia `book` — usado tanto na
/// partida quanto em qualquer re-sincronização (gap detectado, reconexão,
/// ou refresh periódico agendado).
async fn resnapshot(
    depth_client: &BinanceDepthClient,
    symbol_wire: &str,
    instrument_id: InstrumentId,
    depth_limit: u32,
    data_dir: &Path,
    book: &mut LocalOrderBook,
) -> Result<(), MicrostructureError> {
    let snapshot = depth_client
        .fetch_depth_snapshot(symbol_wire, instrument_id, depth_limit)
        .await?;
    let rows = depth_snapshot_to_rows(&snapshot, snapshot.captured_at.timestamp_millis());
    let dir = store::day_dir(
        data_dir,
        symbol_wire,
        "book_snapshots",
        snapshot.captured_at.date_naive(),
    );
    store::write_book_snapshot(&dir, snapshot.captured_at, &rows)?;
    book.seed(&snapshot);
    Ok(())
}

/// Aplica `delta` a `book`; se um gap de sequência for detectado, loga em
/// `desync_log` e re-sincroniza via um snapshot REST novo — o mesmo
/// tratamento de gap usado tanto durante o replay do buffer inicial quanto
/// no loop principal (ver `run_symbol`), fatorado aqui para não duplicar a
/// lógica de recuperação.
#[allow(clippy::too_many_arguments)]
async fn apply_delta_or_resync(
    depth_client: &BinanceDepthClient,
    symbol_wire: &str,
    instrument_id: InstrumentId,
    depth_limit: u32,
    data_dir: &Path,
    book: &mut LocalOrderBook,
    delta: &OrderBookDelta,
) -> Result<()> {
    match book.apply_delta(delta) {
        Ok(_) => {}
        Err(MicrostructureError::SequenceGap { expected, got }) => {
            tracing::warn!(symbol = %symbol_wire, expected, got, "sequence gap detected, resyncing");
            desync_log::append(
                data_dir,
                &DesyncEvent {
                    symbol: symbol_wire.to_string(),
                    detected_at: Utc::now(),
                    kind: DesyncKind::SequenceGap { expected, got },
                    note: "gap detectado ao aplicar delta; re-semeando".to_string(),
                },
            )?;
            if let Err(err) = resnapshot(
                depth_client,
                symbol_wire,
                instrument_id,
                depth_limit,
                data_dir,
                book,
            )
            .await
            {
                tracing::error!(symbol = %symbol_wire, error = %err, "failed to resync after gap");
            }
        }
        Err(other) => {
            tracing::error!(symbol = %symbol_wire, error = %other, "unexpected error applying delta");
        }
    }
    Ok(())
}

#[derive(Default)]
struct Buffers {
    trades: Vec<TradeRow>,
    ticker: Vec<BookTickerRow>,
    deltas: Vec<BookDeltaRow>,
}

impl Buffers {
    fn len(&self) -> usize {
        self.trades.len() + self.ticker.len() + self.deltas.len()
    }

    fn flush(&mut self, data_dir: &Path, symbol_wire: &str) -> Result<(), MicrostructureError> {
        let now = Utc::now();
        let date = now.date_naive();
        if !self.trades.is_empty() {
            let dir = store::day_dir(data_dir, symbol_wire, "trades", date);
            store::write_trades(&dir, now, &self.trades)?;
            self.trades.clear();
        }
        if !self.ticker.is_empty() {
            let dir = store::day_dir(data_dir, symbol_wire, "book_ticker", date);
            store::write_book_ticker(&dir, now, &self.ticker)?;
            self.ticker.clear();
        }
        if !self.deltas.is_empty() {
            let dir = store::day_dir(data_dir, symbol_wire, "book_deltas", date);
            store::write_book_deltas(&dir, now, &self.deltas)?;
            self.deltas.clear();
        }
        Ok(())
    }
}

async fn run_symbol(
    base: String,
    quote: String,
    args: std::sync::Arc<Args>,
    mut shutdown: mpsc::UnboundedReceiver<()>,
) -> Result<()> {
    let symbol_wire = wire_symbol(&base, &quote);
    let instrument_id = microstructure::deterministic_instrument_id(&symbol_wire);
    tracing::info!(symbol = %symbol_wire, ?instrument_id, "starting microstructure collector");

    let depth_client = BinanceDepthClient::public();
    let mut book = LocalOrderBook::new(instrument_id);
    let mut buffers = Buffers::default();

    // O snapshot REST só é buscado depois que o handshake do WS é
    // confirmado (`MicrostructureWsEvent::Connected`) — não basta abrir o
    // stream antes de disparar a chamada REST no código: o handshake
    // (TCP+TLS+inscrição) é bem mais lento que um GET simples, então a
    // chamada REST pode terminar *antes* do WS sequer conectar, deixando
    // um buraco de dado entre o `lastUpdateId` do snapshot e o primeiro
    // delta recebido — um gap garantido, exatamente o que uma rodada de
    // teste real contra a Binance expôs mesmo já com o stream "aberto"
    // primeiro no código. Esperar `Connected` fecha a corrida: a ordem de
    // entrega do canal garante que nenhum `Depth`/`BookTicker`/`Trade`
    // chega antes dele (ver o doc de `Connected`).
    let ws_client = BinanceMicrostructureWsClient::public();
    let mut rx = ws_client.spawn_stream(symbol_wire.clone(), instrument_id);

    loop {
        match rx.recv().await {
            Some(MicrostructureWsEvent::Connected) => break,
            Some(_) => {
                // Não deveria acontecer antes de `Connected` (ver garantia
                // de ordenação no doc do evento); ignorado com segurança.
            }
            None => {
                return Err(anyhow::anyhow!(
                    "websocket channel closed before connecting for {symbol_wire}"
                ));
            }
        }
    }

    // Com o WS já conectado, bufferiza deltas crus enquanto a chamada REST
    // está em voo; trades/bookTicker recebidos aqui já vão para os buffers
    // normais de persistência.
    let mut pending_deltas: Vec<OrderBookDelta> = Vec::new();
    let snapshot = {
        let snapshot_fut =
            depth_client.fetch_depth_snapshot(&symbol_wire, instrument_id, args.depth_limit);
        tokio::pin!(snapshot_fut);
        loop {
            tokio::select! {
                biased;
                result = &mut snapshot_fut => {
                    break result.with_context(|| format!("seeding initial snapshot for {symbol_wire}"))?;
                }
                event = rx.recv() => {
                    match event {
                        Some(MicrostructureWsEvent::Depth(delta)) => {
                            buffers.deltas.extend(delta_to_rows(&delta));
                            pending_deltas.push(delta);
                        }
                        Some(MicrostructureWsEvent::BookTicker(ticker)) => {
                            buffers.ticker.push(ticker_to_row(&ticker));
                        }
                        Some(MicrostructureWsEvent::Trade(trade)) => {
                            buffers.trades.push(trade_to_row(&trade));
                        }
                        Some(MicrostructureWsEvent::Connected) => {
                            // Reconexão em pleno buffering inicial (raro,
                            // mas possível se a conexão cair de novo bem
                            // no começo); o buffer acumulado até aqui pode
                            // ter um buraco correspondente à queda, então
                            // é descartado — o snapshot que está a
                            // caminho ainda semeará o livro corretamente,
                            // só o buffer não é mais confiável.
                            pending_deltas.clear();
                            buffers.deltas.clear();
                        }
                        None => {
                            return Err(anyhow::anyhow!(
                                "websocket channel closed before initial snapshot for {symbol_wire}"
                            ));
                        }
                    }
                }
            }
        }
    };

    let rows = depth_snapshot_to_rows(&snapshot, snapshot.captured_at.timestamp_millis());
    let dir = store::day_dir(
        &args.data_dir,
        &symbol_wire,
        "book_snapshots",
        snapshot.captured_at.date_naive(),
    );
    store::write_book_snapshot(&dir, snapshot.captured_at, &rows)?;
    book.seed(&snapshot);
    tracing::info!(
        symbol = %symbol_wire,
        last_update_id = ?book.last_update_id(),
        buffered_deltas = pending_deltas.len(),
        "seeded initial book"
    );

    for delta in &pending_deltas {
        apply_delta_or_resync(
            &depth_client,
            &symbol_wire,
            instrument_id,
            args.depth_limit,
            &args.data_dir,
            &mut book,
            delta,
        )
        .await?;
    }

    let mut flush_ticker = tokio::time::interval(StdDuration::from_secs(args.flush_interval_secs));
    let mut resnapshot_ticker =
        tokio::time::interval(StdDuration::from_secs(args.resnapshot_interval_secs));
    flush_ticker.tick().await; // primeiro tick é imediato; descarta.
    resnapshot_ticker.tick().await;

    loop {
        tokio::select! {
            _ = shutdown.recv() => {
                tracing::info!(symbol = %symbol_wire, "shutdown requested, flushing and exiting");
                buffers.flush(&args.data_dir, &symbol_wire)?;
                return Ok(());
            }
            _ = flush_ticker.tick() => {
                buffers.flush(&args.data_dir, &symbol_wire)?;
            }
            _ = resnapshot_ticker.tick() => {
                if let Err(err) = resnapshot(&depth_client, &symbol_wire, instrument_id, args.depth_limit, &args.data_dir, &mut book).await {
                    tracing::warn!(symbol = %symbol_wire, error = %err, "scheduled resnapshot failed, will retry next interval");
                } else {
                    desync_log::append(&args.data_dir, &DesyncEvent {
                        symbol: symbol_wire.clone(),
                        detected_at: Utc::now(),
                        kind: DesyncKind::ScheduledResnapshot,
                        note: "refresh periódico de segurança".to_string(),
                    })?;
                }
            }
            event = rx.recv() => {
                let Some(event) = event else {
                    tracing::warn!(symbol = %symbol_wire, "websocket channel closed, exiting collector for this symbol");
                    buffers.flush(&args.data_dir, &symbol_wire)?;
                    return Ok(());
                };
                match event {
                    MicrostructureWsEvent::Depth(delta) => {
                        buffers.deltas.extend(delta_to_rows(&delta));
                        apply_delta_or_resync(&depth_client, &symbol_wire, instrument_id, args.depth_limit, &args.data_dir, &mut book, &delta).await?;
                    }
                    MicrostructureWsEvent::BookTicker(ticker) => {
                        buffers.ticker.push(ticker_to_row(&ticker));
                    }
                    MicrostructureWsEvent::Trade(trade) => {
                        buffers.trades.push(trade_to_row(&trade));
                    }
                    MicrostructureWsEvent::Connected => {
                        tracing::warn!(symbol = %symbol_wire, "websocket reconnected, resyncing book");
                        desync_log::append(&args.data_dir, &DesyncEvent {
                            symbol: symbol_wire.clone(),
                            detected_at: Utc::now(),
                            kind: DesyncKind::WsReconnect { reason: "stream reconnected".to_string() },
                            note: "re-semeando após reconexão".to_string(),
                        })?;
                        if let Err(err) = resnapshot(&depth_client, &symbol_wire, instrument_id, args.depth_limit, &args.data_dir, &mut book).await {
                            tracing::error!(symbol = %symbol_wire, error = %err, "failed to resync after reconnect");
                        }
                    }
                }

                if buffers.len() >= args.flush_max_rows {
                    buffers.flush(&args.data_dir, &symbol_wire)?;
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = std::sync::Arc::new(Args::parse());
    let pairs = parse_pairs(&args.symbols)?;

    let mut handles = Vec::new();
    let mut shutdown_txs = Vec::new();
    for (base, quote) in pairs {
        let (shutdown_tx, shutdown_rx) = mpsc::unbounded_channel();
        shutdown_txs.push(shutdown_tx);
        let args = args.clone();
        handles.push(tokio::spawn(run_symbol(base, quote, args, shutdown_rx)));
    }

    tokio::signal::ctrl_c()
        .await
        .context("waiting for ctrl-c")?;
    tracing::info!("ctrl-c received, shutting down collectors");
    for tx in &shutdown_txs {
        let _ = tx.send(());
    }

    for handle in handles {
        if let Err(err) = handle.await.context("collector task panicked")? {
            tracing::error!(error = %err, "collector task exited with error");
        }
    }

    Ok(())
}
