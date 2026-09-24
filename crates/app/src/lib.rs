//! A metade "biblioteca" do binário `quant-engine`: carregamento de
//! configuração, montagem de instrumentos e estratégias, e o event loop
//! live. Separada do `main.rs` (um entry point enxuto) para que os testes
//! de integração — ver `tests/smoke_test.rs` — possam exercitar a montagem
//! real de ponta a ponta sem duplicá-la.

pub mod config;
pub mod judge_codec;
pub mod pipeline;
pub mod robot_market;
pub mod robot_runtime;
pub mod setup;
pub mod strategy_switch;
