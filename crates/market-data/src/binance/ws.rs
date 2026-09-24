use std::collections::HashMap;
use std::time::Duration;

use domain::{Instrument, InstrumentId, MarketEvent, Timeframe};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};

use crate::binance::dto::{CombinedStreamEnvelope, WsEvent};
use crate::binance::normalize::{ws_kline_to_candle, ws_trade_to_market_trade};
use crate::binance::symbol::instrument_wire_symbol;

const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Dados de mercado da Binance ao vivo pelo endpoint WebSocket de combined
/// stream, com reconexão automática.
///
/// Uma conexão perdida (instabilidade de rede, desconexão do lado da
/// exchange, frame malformado) é tratada como recuperável: o cliente
/// reconecta com backoff exponencial limitado e retoma o streaming. Ele
/// nunca para silenciosamente sem que o chamador descarte o receptor ou o
/// processo termine — toda desconexão/reconexão é registrada em log.
pub struct BinanceWsClient {
    base_url: String,
}

impl BinanceWsClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn public() -> Self {
        Self::new("wss://stream.binance.com:9443")
    }

    /// Dispara uma task em segundo plano que faz streaming de klines e
    /// trades de `instruments` no `timeframe`, retornando a ponta receptora
    /// de um canal ilimitado. A task encerra quando o receptor é
    /// descartado.
    pub fn spawn_stream(
        &self,
        instruments: Vec<Instrument>,
        timeframe: Timeframe,
    ) -> mpsc::UnboundedReceiver<MarketEvent> {
        let (tx, rx) = mpsc::unbounded_channel();

        let symbol_index: HashMap<String, InstrumentId> = instruments
            .iter()
            .map(|i| (instrument_wire_symbol(i), i.id))
            .collect();

        let streams: Vec<String> = instruments
            .iter()
            .flat_map(|i| {
                let symbol = instrument_wire_symbol(i).to_lowercase();
                vec![
                    format!("{symbol}@kline_{}", timeframe.as_str()),
                    format!("{symbol}@trade"),
                ]
            })
            .collect();
        let ws_url = format!("{}/stream?streams={}", self.base_url, streams.join("/"));

        tokio::spawn(run_reconnect_loop(ws_url, symbol_index, timeframe, tx));

        rx
    }
}

async fn run_reconnect_loop(
    ws_url: String,
    symbol_index: HashMap<String, InstrumentId>,
    timeframe: Timeframe,
    tx: mpsc::UnboundedSender<MarketEvent>,
) {
    let mut backoff = INITIAL_BACKOFF;

    loop {
        info!(url = %ws_url, "connecting to Binance websocket");
        match tokio_tungstenite::connect_async(&ws_url).await {
            Ok((stream, _response)) => {
                backoff = INITIAL_BACKOFF;
                info!("Binance websocket connected");

                let (mut write, mut read) = stream.split();
                loop {
                    let Some(message) = read.next().await else {
                        warn!("Binance websocket stream ended");
                        break;
                    };
                    match message {
                        Ok(Message::Text(text)) => {
                            if !handle_text_message(&text, &symbol_index, timeframe, &tx) {
                                // Receptor descartado: encerra o streaming
                                // por completo.
                                return;
                            }
                        }
                        Ok(Message::Ping(payload)) => {
                            if write.send(Message::Pong(payload)).await.is_err() {
                                warn!("failed to send pong, reconnecting");
                                break;
                            }
                        }
                        Ok(Message::Close(frame)) => {
                            warn!(?frame, "Binance websocket closed by server");
                            break;
                        }
                        Ok(_) => {}
                        Err(err) => {
                            error!(error = %err, "Binance websocket read error");
                            break;
                        }
                    }
                }
            }
            Err(err) => {
                error!(error = %err, "failed to connect to Binance websocket");
            }
        }

        if tx.is_closed() {
            info!("receiver dropped, stopping Binance websocket client");
            return;
        }

        warn!(delay_secs = backoff.as_secs(), "reconnecting after backoff");
        tokio::time::sleep(backoff).await;
        backoff = std::cmp::min(backoff * 2, MAX_BACKOFF);
    }
}

/// Faz o parse de um frame de texto e encaminha os `MarketEvent`s
/// resultantes. Retorna `false` se a ponta receptora foi descartada
/// (sinalizando ao chamador que ele deve parar toda a task de streaming), e
/// `true` caso contrário — inclusive quando o parse do frame falha, já que
/// uma única mensagem malformada não deve derrubar a conexão.
fn handle_text_message(
    text: &str,
    symbol_index: &HashMap<String, InstrumentId>,
    timeframe: Timeframe,
    tx: &mpsc::UnboundedSender<MarketEvent>,
) -> bool {
    let envelope: CombinedStreamEnvelope = match serde_json::from_str(text) {
        Ok(envelope) => envelope,
        Err(err) => {
            debug!(error = %err, "failed to parse websocket message, skipping");
            return true;
        }
    };

    let event = match &envelope.data {
        WsEvent::Kline(payload) => {
            let Some(&instrument_id) = symbol_index.get(&payload.symbol) else {
                debug!(symbol = %payload.symbol, "kline for unknown instrument, skipping");
                return true;
            };
            match ws_kline_to_candle(&payload.kline, instrument_id, timeframe) {
                Ok(candle) => MarketEvent::Candle(candle),
                Err(err) => {
                    debug!(error = %err, "failed to normalize kline, skipping");
                    return true;
                }
            }
        }
        WsEvent::Trade(payload) => {
            let Some(&instrument_id) = symbol_index.get(&payload.symbol) else {
                debug!(symbol = %payload.symbol, "trade for unknown instrument, skipping");
                return true;
            };
            match ws_trade_to_market_trade(payload, instrument_id) {
                Ok(trade) => MarketEvent::Trade(trade),
                Err(err) => {
                    debug!(error = %err, "failed to normalize trade, skipping");
                    return true;
                }
            }
        }
    };

    tx.send(event).is_ok()
}
