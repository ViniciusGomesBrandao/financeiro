//! Formatos de transmissão específicos da Binance para microestrutura
//! (REST `depth`, streams `depth`/`bookTicker`/`trade`). Nada neste módulo
//! é exposto fora de `binance/` — `normalize.rs` é o único código
//! autorizado a converter estes tipos em tipos de `domain`/`book`.

use serde::Deserialize;

/// Resposta de `GET /api/v3/depth`.
#[derive(Debug, Deserialize)]
pub struct DepthSnapshotResponse {
    #[serde(rename = "lastUpdateId")]
    pub last_update_id: i64,
    pub bids: Vec<(String, String)>,
    pub asks: Vec<(String, String)>,
}

/// Payload do evento `depthUpdate` (diff depth), Spot. `symbol` não é lido
/// hoje (cada conexão WS já é aberta para um único símbolo, roteado pelo
/// sufixo de `stream`), mantido tipado para debug/log futuro — mesmo
/// motivo de campos não usados em `market_data::binance::dto::RawKline`.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct DepthUpdatePayload {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "U")]
    pub first_update_id: i64,
    #[serde(rename = "u")]
    pub final_update_id: i64,
    #[serde(rename = "b")]
    pub bids: Vec<(String, String)>,
    #[serde(rename = "a")]
    pub asks: Vec<(String, String)>,
}

/// Payload do stream `bookTicker` — sem campo de horário de evento (a
/// própria Binance não inclui um para este stream); `normalize` usa o
/// horário de recebimento local, documentado explicitamente lá.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct BookTickerPayload {
    #[serde(rename = "u")]
    pub update_id: i64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "b")]
    pub bid_price: String,
    #[serde(rename = "B")]
    pub bid_qty: String,
    #[serde(rename = "a")]
    pub ask_price: String,
    #[serde(rename = "A")]
    pub ask_qty: String,
}

/// Payload do stream `trade` — mesmos campos de
/// `market_data::binance::dto::TradeEventPayload`, redefinidos aqui porque
/// aquele módulo é privado ao crate `market-data` (ver o doc do crate raiz
/// sobre por que `microstructure` não reaproveita o cliente WS de
/// `market-data`).
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct TradePayload {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "t")]
    pub trade_id: i64,
    #[serde(rename = "p")]
    pub price: String,
    #[serde(rename = "q")]
    pub quantity: String,
    #[serde(rename = "T")]
    pub trade_time: i64,
    #[serde(rename = "m")]
    pub buyer_is_maker: bool,
}

/// O envelope com que a Binance embrulha toda mensagem quando a conexão é
/// feita no endpoint de combined stream (`/stream?streams=...`). `data`
/// fica cru (`serde_json::Value`) porque o `bookTicker` **não** tem o
/// campo discriminador `"e"` que `depthUpdate`/`trade` têm — o roteamento
/// para o tipo certo usa o sufixo de `stream` (`@depth`/`@bookTicker`/`@trade`),
/// não um enum internamente tagueado. Ver `ws.rs::route_message`.
#[derive(Debug, Deserialize)]
pub struct CombinedStreamEnvelope {
    pub stream: String,
    pub data: serde_json::Value,
}
