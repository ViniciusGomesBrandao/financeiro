//! Feature Engine: cálculo de indicadores/features reutilizáveis, separado
//! de qualquer estratégia. Estratégias devem *consumir* `FeatureSnapshot` —
//! não recalcular indicadores por conta própria. As três estratégias
//! genéricas existentes (`strategies::generic::{ema_crossover, momentum,
//! mean_reversion}`) ainda calculam seus próprios indicadores internamente
//! e não foram alteradas por este crate — migrá-las para consumir
//! `FeatureEngine` é um passo futuro deliberadamente fora deste trabalho
//! (ver `README.md`).
//!
//! ## Sem look-ahead bias, por construção
//!
//! `FeatureEngine::update` consome exatamente **um** `Candle` fechado por
//! chamada e só enxerga esse candle e os anteriores que já foram
//! alimentados — nunca um futuro. Isso espelha a mesma disciplina que
//! `app::pipeline`/`backtest::BacktestRunner` já aplicam ao despachar
//! candles para `strategies::Strategy` (sempre em ordem cronológica, sempre
//! só candles com `is_closed == true`): alimentar candles fora de ordem, ou
//! candles ainda abertos, é responsabilidade do chamador evitar — o motor
//! não tenta detectar isso, do mesmo jeito que `StrategyRegistry::dispatch`
//! também não tenta. Um `FeatureEngine` é sempre escopado a um único
//! instrumento (mesma convenção dos indicadores internos de
//! `strategies::generic`); orquestrar um por instrumento é responsabilidade
//! de quem chama (`backtest::BacktestRunner` faz isso — ver `crates/backtest`).
//!
//! ## Só OHLCV, por enquanto
//!
//! Tudo aqui é derivado de candles (open/high/low/close/volume). Um livro
//! de ordens ou uma sequência de trades individuais carrega informação que
//! OHLCV *não* captura (profundidade, desequilíbrio de fluxo de ordens,
//! tamanho médio de trade, ...) — fingir que dá para aproximar isso a
//! partir de candles seria enganoso. Um futuro módulo de features de
//! microestrutura (`MarketTrade`/`OrderBookSnapshot`, já existentes em
//! `domain`) deve viver como um crate ou módulo **irmão** deste, com seu
//! próprio `FeatureSnapshot` — não uma extensão de `ohlcv::FeatureSnapshot`
//! com campos "às vezes preenchidos, às vezes não" dependendo da fonte de
//! dados disponível.
//!
//! ## `f64`, não `Decimal`
//!
//! Assim como `strategies::generic::indicators::Ema`, todo valor aqui é
//! `f64`: são leituras estatísticas derivadas (médias, desvios,
//! correlações, ...), não valores monetários — a exatidão decimal de
//! `rust_decimal` não é necessária, e `f64` mantém a aritmética (raiz
//! quadrada, regressão, EWMA) simples e rápida. A conversão de `Decimal`
//! para `f64` acontece uma vez, na borda (`FeatureEngine::update`).

pub mod config;
pub mod engine;
pub mod ohlcv;
pub mod primitives;
pub mod snapshot;

pub use config::FeatureConfig;
pub use engine::FeatureEngine;
pub use snapshot::{BollingerBands, FeatureSnapshot, RollingRegression};
