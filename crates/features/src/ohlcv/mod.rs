//! Features derivadas de OHLCV (open/high/low/close/volume) — a única
//! fonte de dados suportada por este crate hoje. Cada submódulo é uma
//! família de feature: um "calculador" incremental (struct com `update`)
//! mais a fórmula/conceito e limitações documentados no topo do arquivo.
//!
//! Um futuro módulo de features de microestrutura (`domain::MarketTrade`/
//! `domain::OrderBookSnapshot`) deve ser um módulo **irmão** deste
//! (`microstructure`, não implementado aqui) — ver o doc do crate raiz para
//! o porquê de não misturar as duas fontes num único `FeatureSnapshot`.

pub mod atr;
pub mod autocorrelation;
pub mod bollinger;
pub mod momentum;
pub mod moving_average;
pub mod regression;
pub mod returns;
pub mod rsi;
pub mod volatility;
pub mod volume;
pub mod zscore;
