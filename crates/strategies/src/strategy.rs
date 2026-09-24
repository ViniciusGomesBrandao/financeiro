use domain::{Instrument, MarketEvent, Signal, StrategyId};

use crate::position_query::PositionQuery;
use crate::requirements::StrategyRequirements;

/// Uma estratégia quantitativa: transforma dados de mercado em opiniões
/// (`Signal`s).
///
/// Uma implementação de `Strategy` não deve:
/// - enviar, modificar ou cancelar ordens (isso é papel do broker, alcançado
///   apenas através do motor de risco);
/// - ler ou escrever no banco de dados (isso é papel da camada de
///   persistência);
/// - chamar uma exchange diretamente (isso é papel da camada de market-data /
///   execução);
/// - verificar por conta própria as sessões de negociação pelo relógio (isso é
///   papel de um `TradingCalendar`, aplicado por quem conduz a estratégia);
/// - manter sua própria cópia de "estou posicionada" — quando a decisão
///   depende disso, deve consultar `positions` (ver `PositionQuery`), nunca
///   assumir que um sinal de entrada emitido virou posição de fato.
///
/// Uma estratégia *pode* manter estado interno (ex. uma EMA acumulada) entre
/// chamadas — espera-se que as implementações sejam `struct`s com estado, não
/// funções livres.
pub trait Strategy: Send {
    fn id(&self) -> &StrategyId;

    /// O contrato de dados e de compatibilidade de mercado da estratégia. Deve
    /// ser estável por todo o tempo de vida da instância da estratégia — é
    /// verificado uma vez no registro, não a cada evento.
    fn requirements(&self) -> &StrategyRequirements;

    /// Processa um evento de mercado para um instrumento e opcionalmente emite
    /// um sinal. Chamado somente para instrumentos contra os quais a estratégia
    /// foi registrada com sucesso (veja `registry::StrategyRegistry`), e somente
    /// com tipos de evento presentes em
    /// `requirements().required_market_data`.
    ///
    /// `positions` é a única fonte de verdade sobre posição real — quem
    /// chama passa o `portfolio::PortfolioManager` de produção/backtest (ou
    /// um stub em teste); uma implementação nunca deve substituir isto por
    /// estado próprio.
    fn on_event(
        &mut self,
        instrument: &Instrument,
        event: &MarketEvent,
        positions: &dyn PositionQuery,
    ) -> Option<Signal>;
}
