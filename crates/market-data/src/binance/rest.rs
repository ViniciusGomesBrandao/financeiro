use std::str::FromStr;

use chrono::{DateTime, Utc};
use domain::{
    Asset, AssetClass, Candle, Exchange, Instrument, InstrumentId, MarketType, Timeframe,
};
use rust_decimal::Decimal;
use tracing::debug;

use crate::binance::dto::{ExchangeInfoResponse, RawKline, SymbolFilter};
use crate::binance::normalize::raw_kline_to_candle;
use crate::binance::symbol::to_wire_symbol;
use crate::error::MarketDataError;

/// Wrapper fino sobre os endpoints REST públicos do Binance Spot. Todo
/// método retorna tipos de `domain` (ou um `MarketDataError`) — nenhum
/// formato de JSON da Binance escapa deste módulo.
pub struct BinanceRestClient {
    http: reqwest::Client,
    base_url: String,
}

impl BinanceRestClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
        }
    }

    pub fn public() -> Self {
        Self::new("https://api.binance.com")
    }

    pub async fn fetch_klines(
        &self,
        instrument_id: InstrumentId,
        base: &Asset,
        quote: &Asset,
        timeframe: Timeframe,
        limit: u32,
    ) -> Result<Vec<Candle>, MarketDataError> {
        let symbol = to_wire_symbol(base, quote);
        let url = format!("{}/api/v3/klines", self.base_url);
        debug!(symbol = %symbol, interval = %timeframe.as_str(), limit, "fetching klines");

        let response = self
            .http
            .get(&url)
            .query(&[
                ("symbol", symbol.as_str()),
                ("interval", timeframe.as_str()),
                ("limit", &limit.to_string()),
            ])
            .send()
            .await
            .map_err(|source| MarketDataError::Http {
                url: url.clone(),
                source,
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(MarketDataError::ExchangeError(format!(
                "GET {url} -> {status}: {body}"
            )));
        }

        let raw_klines: Vec<RawKline> = response
            .json()
            .await
            .map_err(|source| MarketDataError::Http { url, source })?;

        raw_klines
            .iter()
            .map(|raw| raw_kline_to_candle(raw, instrument_id, timeframe))
            .collect()
    }

    /// Como `fetch_klines`, mas para um intervalo de tempo explícito em vez
    /// das `limit` barras mais recentes — a primitiva de paginação que o
    /// download de histórico em massa (`historical-data`) usa para
    /// reconstruir meses/anos de candles, uma página de até `limit` (máx.
    /// 1000, limite da própria Binance) por chamada. `end_time` ausente
    /// significa "até agora".
    #[allow(clippy::too_many_arguments)]
    pub async fn fetch_klines_range(
        &self,
        instrument_id: InstrumentId,
        base: &Asset,
        quote: &Asset,
        timeframe: Timeframe,
        start_time: DateTime<Utc>,
        end_time: Option<DateTime<Utc>>,
        limit: u32,
    ) -> Result<Vec<Candle>, MarketDataError> {
        let symbol = to_wire_symbol(base, quote);
        let url = format!("{}/api/v3/klines", self.base_url);
        let start_ms = start_time.timestamp_millis().to_string();
        let limit_str = limit.to_string();
        debug!(
            symbol = %symbol,
            interval = %timeframe.as_str(),
            start = %start_time,
            end = ?end_time,
            limit,
            "fetching historical klines range"
        );

        let mut query = vec![
            ("symbol", symbol.as_str()),
            ("interval", timeframe.as_str()),
            ("startTime", start_ms.as_str()),
            ("limit", limit_str.as_str()),
        ];
        let end_ms = end_time.map(|t| t.timestamp_millis().to_string());
        if let Some(end_ms) = &end_ms {
            query.push(("endTime", end_ms.as_str()));
        }

        let response = self
            .http
            .get(&url)
            .query(&query)
            .send()
            .await
            .map_err(|source| MarketDataError::Http {
                url: url.clone(),
                source,
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(MarketDataError::ExchangeError(format!(
                "GET {url} -> {status}: {body}"
            )));
        }

        let raw_klines: Vec<RawKline> = response
            .json()
            .await
            .map_err(|source| MarketDataError::Http { url, source })?;

        raw_klines
            .iter()
            .map(|raw| raw_kline_to_candle(raw, instrument_id, timeframe))
            .collect()
    }

    pub async fn fetch_instrument(
        &self,
        base: &Asset,
        quote: &Asset,
    ) -> Result<Instrument, MarketDataError> {
        let symbol = to_wire_symbol(base, quote);
        let url = format!("{}/api/v3/exchangeInfo", self.base_url);

        let response = self
            .http
            .get(&url)
            .query(&[("symbol", symbol.as_str())])
            .send()
            .await
            .map_err(|source| MarketDataError::Http {
                url: url.clone(),
                source,
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(MarketDataError::ExchangeError(format!(
                "GET {url} -> {status}: {body}"
            )));
        }

        let info: ExchangeInfoResponse = response
            .json()
            .await
            .map_err(|source| MarketDataError::Http { url, source })?;

        let symbol_info = info
            .symbols
            .into_iter()
            .find(|s| s.symbol == symbol)
            .ok_or_else(|| MarketDataError::InstrumentNotFound(symbol.clone()))?;

        let mut tick_size = Decimal::from_str("0.01").unwrap();
        let mut lot_size = Decimal::from_str("0.0001").unwrap();
        let mut min_qty = Decimal::from_str("0.0001").unwrap();
        let mut min_notional = Decimal::from_str("10").unwrap();

        for filter in &symbol_info.filters {
            match filter {
                SymbolFilter::PriceFilter { tick_size: ts } => {
                    if let Ok(v) = Decimal::from_str(ts) {
                        tick_size = v;
                    }
                }
                SymbolFilter::LotSize {
                    step_size,
                    min_qty: mq,
                } => {
                    if let Ok(v) = Decimal::from_str(step_size) {
                        lot_size = v;
                    }
                    if let Ok(v) = Decimal::from_str(mq) {
                        min_qty = v;
                    }
                }
                SymbolFilter::MinNotional { min_notional: mn }
                | SymbolFilter::Notional { min_notional: mn } => {
                    if let Ok(v) = Decimal::from_str(mn) {
                        min_notional = v;
                    }
                }
                SymbolFilter::Other => {}
            }
        }

        Ok(Instrument::new(
            domain::Symbol::from_pair(base, quote),
            base.clone(),
            quote.clone(),
            AssetClass::Crypto,
            Exchange::Binance,
            MarketType::Spot,
            tick_size,
            lot_size,
            min_qty,
            min_notional,
        ))
    }
}
