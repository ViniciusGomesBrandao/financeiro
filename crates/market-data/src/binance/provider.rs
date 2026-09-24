use async_trait::async_trait;
use domain::{Asset, AssetClass, Candle, Instrument, MarketDataKind, MarketEvent, Timeframe};
use tokio::sync::mpsc;

use crate::binance::rest::BinanceRestClient;
use crate::binance::ws::BinanceWsClient;
use crate::error::MarketDataError;
use crate::provider::{MarketDataProvider, ProviderCapabilities};

/// `MarketDataProvider` sustentado pelos dados públicos de mercado do
/// Binance Spot.
///
/// Apenas endpoints públicos e sem autenticação são usados (klines
/// recentes, exchange info e os streams WebSocket públicos de kline/trade)
/// — este provider nunca precisa de chaves de API e nunca envia ordens. O
/// envio de ordens é responsabilidade exclusiva do crate `execution` e,
/// mesmo lá, hoje só existe o `PaperBroker`.
pub struct BinanceMarketData {
    rest: BinanceRestClient,
    ws: BinanceWsClient,
}

impl BinanceMarketData {
    pub fn public() -> Self {
        Self {
            rest: BinanceRestClient::public(),
            ws: BinanceWsClient::public(),
        }
    }

    pub fn with_base_urls(
        rest_base_url: impl Into<String>,
        ws_base_url: impl Into<String>,
    ) -> Self {
        Self {
            rest: BinanceRestClient::new(rest_base_url),
            ws: BinanceWsClient::new(ws_base_url),
        }
    }
}

impl Default for BinanceMarketData {
    fn default() -> Self {
        Self::public()
    }
}

#[async_trait]
impl MarketDataProvider for BinanceMarketData {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            market_data_kinds: vec![MarketDataKind::Ohlcv, MarketDataKind::Trades],
            asset_classes: vec![AssetClass::Crypto],
        }
    }

    async fn fetch_recent_candles(
        &self,
        instrument: &Instrument,
        timeframe: Timeframe,
        limit: u32,
    ) -> Result<Vec<Candle>, MarketDataError> {
        self.rest
            .fetch_klines(
                instrument.id,
                &instrument.base_asset,
                &instrument.quote_asset,
                timeframe,
                limit,
            )
            .await
    }

    async fn fetch_instrument(
        &self,
        base: &Asset,
        quote: &Asset,
    ) -> Result<Instrument, MarketDataError> {
        self.rest.fetch_instrument(base, quote).await
    }

    async fn stream(
        &self,
        instruments: Vec<Instrument>,
        timeframe: Timeframe,
    ) -> Result<mpsc::UnboundedReceiver<MarketEvent>, MarketDataError> {
        Ok(self.ws.spawn_stream(instruments, timeframe))
    }
}
