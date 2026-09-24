//! O resultado de alimentar o `MicrostructureFeatureEngine`: um valor (ou
//! `None`, se a janela relevante ainda não tem dado suficiente) por
//! feature — mesma convenção de `features::snapshot::FeatureSnapshot`
//! (nunca inventa um zero, `None` é sempre "ainda não sei", nunca "é
//! zero"). **Este struct não compartilha nenhum campo ou tipo com
//! `features::snapshot::FeatureSnapshot`** — são universos de dado
//! diferentes (book/trade tape vs. candle OHLCV), de propósito.

use chrono::{DateTime, Utc};
use domain::InstrumentId;

/// Todas as features de microestrutura calculadas para um instante — ou o
/// estado do book, ou um trade, mudou. Ver `engine::MicrostructureFeatureEngine`
/// para como cada campo é calculado e `config::MicrostructureConfig` para os
/// parâmetros de janela.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MicrostructureSnapshot {
    pub instrument_id: InstrumentId,
    pub timestamp: DateTime<Utc>,

    /// `melhor_ask.preço - melhor_bid.preço`. `None` até o book ter os dois
    /// lados povoados.
    pub spread_abs: Option<f64>,
    /// `spread_abs / mid_price` — o spread como fração do preço, comparável
    /// entre instrumentos de preços muito diferentes.
    pub spread_pct: Option<f64>,
    /// `(melhor_bid.preço + melhor_ask.preço) / 2` — mesma definição de
    /// `domain::OrderBookSnapshot::mid_price`.
    pub mid_price: Option<f64>,
    /// Preço médio ponderado pelo volume do lado *oposto*:
    /// `(ask.preço·bid.qty + bid.preço·ask.qty) / (bid.qty+ask.qty)`.
    /// Mais quantidade parada no bid "empurra" o microprice em direção ao
    /// ask (a leitura padrão de Stoikov: o lado com mais volume tende a
    /// consumir o lado mais fino primeiro, movendo o preço para lá).
    pub microprice: Option<f64>,
    /// Imbalance no topo do livro (L1):
    /// `(bid.qty - ask.qty) / (bid.qty + ask.qty)`, em `[-1, 1]`. Positivo =
    /// mais quantidade comprando no melhor bid do que vendendo no melhor ask.
    pub bid_ask_imbalance: Option<f64>,
    /// Mesma fórmula do imbalance L1, mas somando a quantidade dos
    /// `depth_levels` melhores níveis de cada lado (`MicrostructureConfig`).
    pub book_imbalance: Option<f64>,
    /// `(volume_comprado - volume_vendido) / (volume_comprado + volume_vendido)`
    /// dos trades na janela rolante (`trade_window_secs`), em `[-1, 1]`.
    /// "Comprado"/"vendido" pelo lado do taker (`MarketTrade::taker_side`).
    pub trade_imbalance: Option<f64>,
    /// `volume_comprado - volume_vendido` na mesma janela, **não
    /// normalizado** (unidades do ativo base) — ao contrário do
    /// `trade_imbalance`, a magnitude aqui importa, não só o sinal.
    pub volume_delta: Option<f64>,
    /// `contagem_de_trades_na_janela / trade_window_secs`, em trades/segundo.
    pub trade_intensity: Option<f64>,
    /// Order-flow imbalance (Cont, Kukanov & Stoikov, 2014) acumulado na
    /// janela `ofi_window_secs` — soma dos incrementos `e` a cada transição
    /// de melhor bid/ask. Ver `engine::order_flow_increment` para a fórmula
    /// exata e um exemplo numérico resolvido.
    pub order_flow_imbalance: Option<f64>,
}
