//! O motor de estratégias: transforma dados de mercado em `Signal`s.
//!
//! A organização reflete a regra arquitetural central do projeto — abstraia
//! somente o que é genuinamente universal entre mercados:
//!
//! - `generic/` — estratégias válidas para múltiplas classes de ativos (hoje:
//!   `Crypto` e `Equity`), porque seu cálculo é genuinamente agnóstico de
//!   mercado (uma EMA de preços de fechamento não se importa com qual é o
//!   instrumento).
//! - `crypto/` — estratégias que dependem de dados ou estrutura de mercado
//!   específicos de cripto (funding rates, dados on-chain, perpétuos).
//! - `equities/` — estratégias que dependem de dados ou estrutura de mercado
//!   específicos de ações (sessões de negociação, eventos corporativos).
//!
//! `requirements` + `registry` são o mecanismo de compatibilidade: toda
//! estratégia declara `StrategyRequirements` (classes de ativos suportadas +
//! tipos de dados de mercado necessários), e `StrategyRegistry::register` se
//! recusa a registrar uma estratégia contra um instrumento que ela não suporta.
//! Não há outro caminho para ligar uma estratégia a um instrumento, então
//! combinações incompatíveis não podem rodar silenciosamente.
//!
//! `feature_strategy` é o mecanismo para uma estratégia consumir
//! `features::FeatureSnapshot` em vez de recalcular indicadores por conta
//! própria — ver o doc do módulo. As três estratégias baseline
//! (`generic::{ema_crossover, momentum, mean_reversion}`) continuam
//! implementando `Strategy` diretamente, com seus próprios indicadores
//! internos, como referência de comparação; as três estratégias
//! quantitativas mais novas (`generic::{statistical_mean_reversion,
//! quant_momentum, volatility_breakout}`) implementam `FeatureStrategy`.
//!
//! `catalog` é o ponto único de "quais estratégias existem" — id →
//! metadados de descoberta (`StrategyDescriptor`) + fábrica de instância
//! independente. `StrategyRegistry` continua sendo o *dispatcher* (quais
//! estratégias já construídas estão aprovadas para quais instrumentos
//! nesta execução) — os dois papéis são deliberadamente separados, ver o
//! doc de `catalog` para o porquê.
//!
//! `instance` é a terceira peça (Fase 1.5): `StrategyInstanceConfig` +
//! `build_registry` transformam uma lista de instâncias configuradas por
//! dado (id + kind do catálogo + símbolos) num `StrategyRegistry` pronto —
//! o que permite múltiplas instâncias independentes da mesma estratégia
//! (ids diferentes) e estratégias diferentes por instrumento, sem nenhuma
//! combinação hardcoded no chamador. Ver o doc do módulo.

pub mod catalog;
pub mod crypto;
pub mod equities;
pub mod error;
pub mod feature_strategy;
pub mod generic;
pub mod instance;
pub mod position_query;
pub mod registry;
pub mod requirements;
pub mod strategy;

pub use catalog::{CatalogError, StrategyCatalogEntry, StrategyCategory, StrategyDescriptor};
pub use error::CompatibilityError;
pub use feature_strategy::{FeatureStrategy, FeatureStrategyAdapter};
pub use instance::{build_registry, InstanceError, StrategyInstanceConfig};
pub use position_query::{NoPositions, PositionQuery};
pub use registry::{check_compatible, StrategyRegistry};
pub use requirements::StrategyRequirements;
pub use strategy::Strategy;
