//! A fonte mínima de verdade sobre posição real que uma estratégia consulta
//! em vez de lembrar por conta própria se acredita estar posicionada.
//!
//! O problema que isto resolve: `on_event`/`on_features` só produz uma
//! *opinião* (`Signal`) — quem decide se ela vira posição de verdade é o
//! `risk::RiskEngine` (que pode rejeitar) seguido do `execution::Broker`/
//! `portfolio::PortfolioManager` (que efetivamente executam). Se a
//! estratégia marcasse "estou posicionada" só por ter emitido um sinal de
//! entrada, uma rejeição do Risk Engine a deixaria presa nesse estado
//! otimista — monitorando a saída de uma posição que nunca existiu, incapaz
//! de detectar uma entrada válida futura. `PositionQuery` existe para que a
//! estratégia nunca precise adivinhar: ela pergunta, a cada candle, ao dono
//! real do estado.
//!
//! Deliberadamente um trait mínimo, não um acoplamento a `execution::Broker`
//! nem a `persistence` — a estratégia não precisa saber *como* uma posição
//! foi aberta, só se ela existe agora.
//!
//! **Multi-robô (ADR-20).** `PositionQuery::open_position_side` recebe só
//! `instrument_id` — de propósito, ele nunca muda: nenhuma das 6
//! estratégias precisa saber que existem outros robôs. O que muda é *quem*
//! responde à pergunta. [`RobotPositions`] é a fonte multi-robô de verdade
//! (implementada sobre `portfolio::PortfolioManager`, que já indexa
//! posições por `(InstrumentId, StrategyId)`); [`StrategyRegistry::dispatch`](crate::registry::StrategyRegistry::dispatch)
//! constrói, para cada estratégia despachada, um [`ForStrategy`] que
//! implementa `PositionQuery` filtrando as respostas de `RobotPositions`
//! pelo `StrategyId` daquela estratégia — assim cada uma só enxerga a
//! própria posição, nunca a de outro robô no mesmo instrumento, sem que o
//! trait que ela consome tenha mudado.

use std::collections::HashMap;

use domain::{InstrumentId, Side, StrategyId};

pub trait PositionQuery {
    /// O lado (`Side::Buy`/`Side::Sell`) da posição real e aberta para este
    /// instrumento, se houver uma. `None` significa "não há posição aberta
    /// agora" — nunca "ainda não sei", já que a fonte é sempre consultada ao
    /// vivo, nunca cacheada pela estratégia.
    fn open_position_side(&self, instrument_id: InstrumentId) -> Option<Side>;
}

/// A fonte multi-robô de verdade sobre posições reais — o equivalente de
/// `PositionQuery` para quem precisa saber a posição de um robô
/// *específico*, não "a" posição de um instrumento. Só `StrategyRegistry::dispatch`
/// e código de infraestrutura equivalente devem consumir isto diretamente;
/// estratégias nunca veem este trait, só `PositionQuery` (via [`ForStrategy`]).
pub trait RobotPositions {
    fn open_position_side_for(
        &self,
        instrument_id: InstrumentId,
        strategy_id: &StrategyId,
    ) -> Option<Side>;
}

impl RobotPositions for portfolio::PortfolioManager {
    fn open_position_side_for(
        &self,
        instrument_id: InstrumentId,
        strategy_id: &StrategyId,
    ) -> Option<Side> {
        self.open_position_for(instrument_id, strategy_id)
            .map(|position| position.side)
    }
}

/// Adapta uma fonte [`RobotPositions`] para [`PositionQuery`], escopada a
/// um único `strategy_id` — é isto que `StrategyRegistry::dispatch`
/// constrói por entrada despachada, para que cada estratégia só veja a
/// própria posição.
pub(crate) struct ForStrategy<'a> {
    pub(crate) source: &'a dyn RobotPositions,
    pub(crate) strategy_id: &'a StrategyId,
}

impl PositionQuery for ForStrategy<'_> {
    fn open_position_side(&self, instrument_id: InstrumentId) -> Option<Side> {
        self.source
            .open_position_side_for(instrument_id, self.strategy_id)
    }
}

/// Implementação baseada em `HashMap`, para testes que precisam simular
/// posição real (inclusive rejeições do Risk Engine, deixando o mapa
/// intocado) sem depender de um `PortfolioManager` completo. Usada
/// diretamente como `PositionQuery` (contra `on_event`/`on_features`, sem
/// passar por `dispatch`) em testes de uma única estratégia; como
/// `RobotPositions` (para testes que passam por `dispatch`), ignora
/// `strategy_id` — válido só quando o teste registra uma única estratégia
/// para o instrumento em questão.
impl PositionQuery for HashMap<InstrumentId, Side> {
    fn open_position_side(&self, instrument_id: InstrumentId) -> Option<Side> {
        self.get(&instrument_id).copied()
    }
}

impl RobotPositions for HashMap<InstrumentId, Side> {
    fn open_position_side_for(
        &self,
        instrument_id: InstrumentId,
        _strategy_id: &StrategyId,
    ) -> Option<Side> {
        self.get(&instrument_id).copied()
    }
}

/// Nunca reporta posição alguma — para testar estratégias/caminhos que não
/// dependem de estado de posição (ex.: as baselines em `generic::{
/// ema_crossover, momentum, mean_reversion}`, que não rastreiam posição).
pub struct NoPositions;

impl PositionQuery for NoPositions {
    fn open_position_side(&self, _instrument_id: InstrumentId) -> Option<Side> {
        None
    }
}

impl RobotPositions for NoPositions {
    fn open_position_side_for(
        &self,
        _instrument_id: InstrumentId,
        _strategy_id: &StrategyId,
    ) -> Option<Side> {
        None
    }
}
