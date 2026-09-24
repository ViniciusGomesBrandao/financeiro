//! Download paginado de histórico de candles da Binance, a partir de
//! `fetch_klines_range` (`market_data::BinanceRestClient`) — a Binance
//! limita cada chamada a no máximo 1000 candles, então uma janela de
//! meses/anos precisa de várias páginas em sequência.

use chrono::{DateTime, Utc};
use domain::{Asset, Candle, InstrumentId, Timeframe};
use market_data::binance::BinanceRestClient;
use tracing::debug;

use crate::error::HistoricalDataError;

const PAGE_LIMIT: u32 = 1000;
/// Pausa entre páginas — a Binance permite bem mais que isso no peso de
/// requisição pública, mas manter um ritmo comedido evita disparar rate
/// limiting numa reprodução de anos de dados em milhares de páginas.
const PAGE_DELAY: std::time::Duration = std::time::Duration::from_millis(200);

/// Baixa todo o histórico disponível de `timeframe` a partir de
/// `start_time` (inclusive) até "agora", paginando em blocos de até 1000
/// candles. Retorna só candles **fechados**, ordenados por `open_time`
/// (a Binance já retorna cada página ordenada; a concatenação de páginas
/// sequenciais preserva isso). Se `start_time` for anterior ao listing do
/// símbolo na Binance, a própria exchange simplesmente começa a responder
/// a partir do primeiro candle real — não há necessidade de conhecer a
/// data de listing de antemão.
pub async fn download_range(
    rest: &BinanceRestClient,
    instrument_id: InstrumentId,
    base: &Asset,
    quote: &Asset,
    timeframe: Timeframe,
    start_time: DateTime<Utc>,
) -> Result<Vec<Candle>, HistoricalDataError> {
    let mut all = Vec::new();
    let mut cursor = start_time;

    loop {
        let raw = rest
            .fetch_klines_range(
                instrument_id,
                base,
                quote,
                timeframe,
                cursor,
                None,
                PAGE_LIMIT,
            )
            .await?;
        let raw_len = raw.len();
        let closed: Vec<Candle> = raw.into_iter().filter(|c| c.is_closed).collect();

        debug!(
            symbol = %base.as_str(),
            cursor = %cursor,
            fetched = raw_len,
            closed = closed.len(),
            "downloaded a page of historical klines"
        );

        if closed.is_empty() {
            break;
        }
        let last_close_time = closed.last().expect("checked non-empty above").close_time;
        all.extend(closed);

        if raw_len < PAGE_LIMIT as usize {
            // A Binance devolveu menos que o pedido: não há mais candles
            // disponíveis depois deste ponto (alcançamos "agora").
            break;
        }
        cursor = last_close_time;
        tokio::time::sleep(PAGE_DELAY).await;
    }

    Ok(all)
}
