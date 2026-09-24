//! Cliente WebSocket de combined stream (`depth@100ms` + `bookTicker` +
//! `trade`) para um único símbolo, com reconexão automática — o mesmo
//! esqueleto de `market_data::binance::ws` (backoff exponencial limitado,
//! um frame malformado nunca derruba a conexão), copiado em vez de
//! reaproveitado: aquele cliente já é especializado em `MarketEvent`
//! (kline+trade) para o pipeline de trading ao vivo, e misturar
//! depth/bookTicker ali acoplaria as duas camadas exatamente onde este
//! projeto pediu separação (ver o doc do crate raiz).

use std::time::Duration;

use chrono::Utc;
use domain::{BookTicker, InstrumentId, MarketTrade, OrderBookDelta};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};

use crate::binance::dto::{
    BookTickerPayload, CombinedStreamEnvelope, DepthUpdatePayload, TradePayload,
};
use crate::binance::normalize::{
    ws_book_ticker_to_domain, ws_depth_update_to_delta, ws_trade_to_market_trade,
};

const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq)]
pub enum MicrostructureWsEvent {
    Depth(OrderBookDelta),
    BookTicker(BookTicker),
    Trade(MarketTrade),
    /// O handshake da conexão (TCP+TLS+inscrição nos streams) terminou —
    /// enviado em **toda** conexão bem-sucedida, inclusive a primeira.
    ///
    /// A ordem de entrega no canal garante que este evento chega antes de
    /// qualquer `Depth`/`BookTicker`/`Trade` daquela conexão (é enviado
    /// antes do loop de leitura de frames começar) — por isso quem chama
    /// pode usá-lo como sinal de "agora é seguro buscar o snapshot REST":
    /// buscar o snapshot *antes* do handshake terminar é exatamente a
    /// corrida que causa um gap garantido na largada (o book já andou
    /// entre a resposta do REST e o momento em que o WS começa a entregar
    /// dado). Na primeira conexão, `Connected` é esse sinal de partida; em
    /// qualquer conexão seguinte (depois de uma queda), é o sinal de que é
    /// preciso re-buscar um snapshot e re-semear, já que qualquer delta
    /// perdido durante a queda torna o `LocalOrderBook` não confiável.
    Connected,
}

pub struct BinanceMicrostructureWsClient {
    base_url: String,
}

impl BinanceMicrostructureWsClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn public() -> Self {
        Self::new("wss://stream.binance.com:9443")
    }

    pub fn spawn_stream(
        &self,
        symbol_wire: String,
        instrument_id: InstrumentId,
    ) -> mpsc::UnboundedReceiver<MicrostructureWsEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        let lower = symbol_wire.to_lowercase();
        let streams = [
            format!("{lower}@depth@100ms"),
            format!("{lower}@bookTicker"),
            format!("{lower}@trade"),
        ];
        let ws_url = format!("{}/stream?streams={}", self.base_url, streams.join("/"));

        tokio::spawn(run_reconnect_loop(ws_url, instrument_id, tx));
        rx
    }
}

async fn run_reconnect_loop(
    ws_url: String,
    instrument_id: InstrumentId,
    tx: mpsc::UnboundedSender<MicrostructureWsEvent>,
) {
    let mut backoff = INITIAL_BACKOFF;

    loop {
        info!(url = %ws_url, "connecting to Binance microstructure websocket");
        match tokio_tungstenite::connect_async(&ws_url).await {
            Ok((stream, _response)) => {
                backoff = INITIAL_BACKOFF;
                info!("Binance microstructure websocket connected");

                if tx.send(MicrostructureWsEvent::Connected).is_err() {
                    return;
                }

                let (mut write, mut read) = stream.split();
                loop {
                    let Some(message) = read.next().await else {
                        warn!("Binance microstructure websocket stream ended");
                        break;
                    };
                    match message {
                        Ok(Message::Text(text)) => {
                            if !handle_text_message(&text, instrument_id, &tx) {
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
                            warn!(?frame, "Binance microstructure websocket closed by server");
                            break;
                        }
                        Ok(_) => {}
                        Err(err) => {
                            error!(error = %err, "Binance microstructure websocket read error");
                            break;
                        }
                    }
                }
            }
            Err(err) => {
                error!(error = %err, "failed to connect to Binance microstructure websocket");
            }
        }

        if tx.is_closed() {
            info!("receiver dropped, stopping Binance microstructure websocket client");
            return;
        }

        warn!(delay_secs = backoff.as_secs(), "reconnecting after backoff");
        tokio::time::sleep(backoff).await;
        backoff = std::cmp::min(backoff * 2, MAX_BACKOFF);
    }
}

/// Faz o parse de um frame e encaminha o `MicrostructureWsEvent`
/// resultante. Retorna `false` se a ponta receptora foi descartada
/// (sinalizando para parar a task inteira), `true` caso contrário —
/// inclusive quando o parse falha, já que um frame malformado não deve
/// derrubar a conexão.
fn handle_text_message(
    text: &str,
    instrument_id: InstrumentId,
    tx: &mpsc::UnboundedSender<MicrostructureWsEvent>,
) -> bool {
    let envelope: CombinedStreamEnvelope = match serde_json::from_str(text) {
        Ok(envelope) => envelope,
        Err(err) => {
            debug!(error = %err, "failed to parse websocket envelope, skipping");
            return true;
        }
    };

    let received_at = Utc::now();
    let event = if envelope.stream.ends_with("@bookTicker") {
        match serde_json::from_value::<BookTickerPayload>(envelope.data) {
            Ok(payload) => ws_book_ticker_to_domain(&payload, instrument_id, received_at)
                .map(MicrostructureWsEvent::BookTicker),
            Err(err) => {
                debug!(error = %err, "failed to parse bookTicker payload, skipping");
                return true;
            }
        }
    } else if envelope.stream.ends_with("@trade") {
        match serde_json::from_value::<TradePayload>(envelope.data) {
            Ok(payload) => {
                ws_trade_to_market_trade(&payload, instrument_id).map(MicrostructureWsEvent::Trade)
            }
            Err(err) => {
                debug!(error = %err, "failed to parse trade payload, skipping");
                return true;
            }
        }
    } else if envelope.stream.contains("@depth") {
        match serde_json::from_value::<DepthUpdatePayload>(envelope.data) {
            Ok(payload) => ws_depth_update_to_delta(&payload, instrument_id, received_at)
                .map(MicrostructureWsEvent::Depth),
            Err(err) => {
                debug!(error = %err, "failed to parse depthUpdate payload, skipping");
                return true;
            }
        }
    } else {
        debug!(stream = %envelope.stream, "unrecognized stream, skipping");
        return true;
    };

    match event {
        Ok(event) => tx.send(event).is_ok(),
        Err(err) => {
            debug!(error = %err, "failed to normalize websocket payload, skipping");
            true
        }
    }
}
