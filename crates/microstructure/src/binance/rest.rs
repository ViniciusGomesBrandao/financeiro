use chrono::Utc;
use domain::InstrumentId;

use crate::binance::dto::DepthSnapshotResponse;
use crate::binance::normalize::depth_snapshot_response_to_snapshot;
use crate::book::DepthSnapshot;
use crate::error::MicrostructureError;

/// Wrapper fino sobre o endpoint REST público de profundidade da Binance
/// Spot — usado para semear (ou re-semear após um gap) um `LocalOrderBook`.
pub struct BinanceDepthClient {
    http: reqwest::Client,
    base_url: String,
}

impl BinanceDepthClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
        }
    }

    pub fn public() -> Self {
        Self::new("https://api.binance.com")
    }

    /// `GET /api/v3/depth`. `limit` deve ser um dos valores aceitos pela
    /// Binance (5/10/20/50/100/500/1000/5000) — o coletor usa 5000 (o
    /// máximo) para ter profundidade suficiente para qualquer diff que
    /// referencie um nível fora do topo do livro.
    pub async fn fetch_depth_snapshot(
        &self,
        symbol_wire: &str,
        instrument_id: InstrumentId,
        limit: u32,
    ) -> Result<DepthSnapshot, MicrostructureError> {
        let url = format!("{}/api/v3/depth", self.base_url);
        let response = self
            .http
            .get(&url)
            .query(&[("symbol", symbol_wire), ("limit", &limit.to_string())])
            .send()
            .await
            .map_err(|source| MicrostructureError::Http {
                url: url.clone(),
                source,
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(MicrostructureError::ExchangeError(format!(
                "GET {url} -> {status}: {body}"
            )));
        }

        let raw: DepthSnapshotResponse = response
            .json()
            .await
            .map_err(|source| MicrostructureError::Http { url, source })?;

        depth_snapshot_response_to_snapshot(&raw, instrument_id, Utc::now())
    }
}
