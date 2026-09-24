//! Estratégias cuja lógica depende apenas de dados universais (OHLCV) e é
//! genuinamente válida entre classes de ativos. Uma estratégia pertence a este
//! módulo somente se declarar `AssetClass::Crypto` *e* `AssetClass::Equity` (ou
//! mais) em seus `StrategyRequirements` por um motivo real — não por padrão. Se
//! uma estratégia só faz sentido para um mercado, ela pertence a `crypto/` ou
//! `equities/`, mesmo que o código tecnicamente compilasse aqui.
//!
//! Duas gerações convivem aqui, lado a lado, de propósito — a segunda não
//! substitui a primeira (ver `crate::feature_strategy` para o mecanismo):
//!
//! - **Baselines** (`ema_crossover`, `momentum`, `mean_reversion`):
//!   implementam `Strategy` diretamente, calculando seus próprios
//!   indicadores via `indicators::{Ema, RollingWindow}`. Mantidas como
//!   estão, sem alteração, como referência simples de comparação.
//! - **Quantitativas** (`statistical_mean_reversion`, `quant_momentum`,
//!   `volatility_breakout`): implementam `FeatureStrategy`, consumindo
//!   `features::FeatureSnapshot` em vez de recalcular indicadores, e
//!   combinando múltiplas features por filtro conjuntivo (nunca por peso
//!   arbitrário) — ver o doc de cada uma para a hipótese e a justificativa
//!   da combinação.

pub mod ema_crossover;
pub mod indicators;
pub mod mean_reversion;
pub mod momentum;
pub mod quant_momentum;
pub mod statistical_mean_reversion;
pub mod volatility_breakout;

pub use ema_crossover::{EmaCrossoverParams, EmaCrossoverStrategy};
pub use mean_reversion::{MeanReversionParams, MeanReversionStrategy};
pub use momentum::{MomentumParams, MomentumStrategy};
pub use quant_momentum::{QuantMomentumParams, QuantMomentumStrategy};
pub use statistical_mean_reversion::{
    StatisticalMeanReversionParams, StatisticalMeanReversionStrategy,
};
pub use volatility_breakout::{VolatilityBreakoutParams, VolatilityBreakoutStrategy};
