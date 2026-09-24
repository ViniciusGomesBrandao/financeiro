//! Formatos de transmissão específicos da Binance. Nada neste módulo é
//! exposto fora do módulo `binance` — `normalize.rs` é o único código
//! autorizado a converter estes tipos em tipos de `domain`.

use serde::Deserialize;

/// Uma linha de `GET /api/v3/klines`. A Binance retorna isso como um array
/// JSON heterogêneo; o serde o desserializa posicionalmente nesta tuple
/// struct. Os campos 7-11 fazem parte do formato fixo do array da Binance,
/// mas hoje não são usados por `normalize::raw_kline_to_candle`; foram
/// mantidos tipados (em vez de colapsados em um único campo genérico) para
/// que o mapeamento posicional dos campos 0-6 permaneça obviamente correto.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct RawKline(
    pub i64,    // 0: horário de abertura (ms)
    pub String, // 1: abertura
    pub String, // 2: máxima
    pub String, // 3: mínima
    pub String, // 4: fechamento
    pub String, // 5: volume
    pub i64,    // 6: horário de fechamento (ms)
    pub String, // 7: volume no ativo de cotação (não usado)
    pub u64,    // 8: número de trades (não usado)
    pub String, // 9: volume de compra do taker no ativo base (não usado)
    pub String, // 10: volume de compra do taker no ativo de cotação (não usado)
    pub String, // 11: não usado
);

#[derive(Debug, Deserialize)]
pub struct ExchangeInfoResponse {
    pub symbols: Vec<SymbolInfo>,
}

#[derive(Debug, Deserialize)]
pub struct SymbolInfo {
    pub symbol: String,
    pub filters: Vec<SymbolFilter>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "filterType")]
pub enum SymbolFilter {
    #[serde(rename = "PRICE_FILTER")]
    PriceFilter {
        #[serde(rename = "tickSize")]
        tick_size: String,
    },
    #[serde(rename = "LOT_SIZE")]
    LotSize {
        #[serde(rename = "stepSize")]
        step_size: String,
        #[serde(rename = "minQty")]
        min_qty: String,
    },
    #[serde(rename = "MIN_NOTIONAL")]
    MinNotional {
        #[serde(rename = "minNotional")]
        min_notional: String,
    },
    #[serde(rename = "NOTIONAL")]
    Notional {
        #[serde(rename = "minNotional")]
        min_notional: String,
    },
    /// Todos os outros tipos de filtro que a Binance define. Precisamos
    /// apenas dos três acima para popular `Instrument`; os demais são
    /// lidos e ignorados, em vez de fazerem a desserialização falhar.
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct KlineEventPayload {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "k")]
    pub kline: KlineData,
}

#[derive(Debug, Deserialize)]
pub struct KlineData {
    #[serde(rename = "t")]
    pub open_time: i64,
    #[serde(rename = "T")]
    pub close_time: i64,
    #[serde(rename = "o")]
    pub open: String,
    #[serde(rename = "h")]
    pub high: String,
    #[serde(rename = "l")]
    pub low: String,
    #[serde(rename = "c")]
    pub close: String,
    #[serde(rename = "v")]
    pub volume: String,
    #[serde(rename = "x")]
    pub is_closed: bool,
}

#[derive(Debug, Deserialize)]
pub struct TradeEventPayload {
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
    /// `true` se o comprador foi o maker — ou seja, este trade foi uma
    /// *venda* do taker.
    #[serde(rename = "m")]
    pub buyer_is_maker: bool,
}

/// O payload `data` de uma mensagem do combined stream, discriminado pelo
/// campo de tipo de evento `"e"` da Binance.
#[derive(Debug, Deserialize)]
#[serde(tag = "e")]
pub enum WsEvent {
    #[serde(rename = "kline")]
    Kline(KlineEventPayload),
    #[serde(rename = "trade")]
    Trade(TradeEventPayload),
}

/// O envelope com que a Binance embrulha toda mensagem quando a conexão é
/// feita no endpoint de combined stream (`/stream?streams=...`).
#[derive(Debug, Deserialize)]
pub struct CombinedStreamEnvelope {
    #[serde(rename = "stream")]
    pub _stream: String,
    pub data: WsEvent,
}
