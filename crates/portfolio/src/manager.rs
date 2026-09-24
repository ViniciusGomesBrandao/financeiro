use std::collections::HashMap;

use chrono::{DateTime, Datelike, Utc};
use domain::{InstrumentId, Money, PortfolioSnapshot, Position, PositionStatus, Side, StrategyId};
use rust_decimal::Decimal;
use tracing::info;
use uuid::Uuid;

use crate::error::PortfolioError;

/// Detém o estado financeiro da simulação: caixa, posições abertas,
/// posições fechadas. Esta é a fonte única de verdade para "o que o
/// portfólio possui no momento" — o motor de risco apenas lê um snapshot
/// dele (`risk_snapshot`), e o paper broker o chama para registrar fills;
/// nenhum dos dois reimplementa essa contabilidade por conta própria.
///
/// As posições são margeadas por colateral de forma uniforme tanto no lado
/// `Buy` quanto no `Sell`: abrir uma posição reserva `entry_price *
/// quantity` (+ fee) de caixa independentemente da direção, e fechá-la
/// devolve esse colateral mais/menos o P&L realizado. Isso evita modelar a
/// mecânica real de empréstimo para short selling, que está fora do escopo
/// de uma fundação de paper trading.
pub struct PortfolioManager {
    initial_cash: Money,
    cash: Money,
    open_positions: Vec<Position>,
    closed_positions: Vec<Position>,
}

impl PortfolioManager {
    pub fn new(initial_cash: Money) -> Self {
        Self {
            initial_cash,
            cash: initial_cash,
            open_positions: Vec::new(),
            closed_positions: Vec::new(),
        }
    }

    /// Reconstrói o estado do portfólio a partir de dados já persistidos no
    /// Postgres — usado no bootstrap do `app` para que um restart nunca
    /// comece com um portfólio "vazio" enquanto o banco já reflete posições
    /// abertas. Quem busca os dados (via `persistence`) é o chamador (o
    /// crate `portfolio` não depende de `persistence`); esta função só
    /// valida e monta o estado em memória a partir do que foi passado.
    ///
    /// `initial_cash` continua sendo o capital configurado (usado para
    /// `return_pct`, exatamente como em `new`); `current_cash` é o caixa
    /// real no momento do restart — tipicamente o `cash` do último
    /// `portfolio_snapshots` persistido, já que ele reflete corretamente
    /// todo o histórico de fees/custos/P&L sem precisar recalculá-lo aqui.
    ///
    /// Não recria, duplica nem altera nenhuma posição: `open_positions` e
    /// `closed_positions` são armazenadas exatamente como foram carregadas
    /// (mesmo `id`, mesmo `entry_price`, mesmo `fees_paid`/`spread_paid`/
    /// `slippage_paid`), preservando P&L e custos para continuidade.
    ///
    /// Rejeita explicitamente (em vez de ignorar) qualquer inconsistência:
    /// mais de uma posição aberta para o mesmo par (instrumento, estratégia)
    /// — o motor de risco assume no máximo uma por robô, não por
    /// instrumento (ver ADR-20) — ou uma posição com `status` diferente do
    /// esperado para a lista em que foi colocada.
    pub fn restore(
        initial_cash: Money,
        current_cash: Money,
        open_positions: Vec<Position>,
        closed_positions: Vec<Position>,
    ) -> Result<Self, PortfolioError> {
        let mut seen_instrument_strategy_pairs = std::collections::HashMap::new();
        for position in &open_positions {
            if position.status != PositionStatus::Open {
                return Err(PortfolioError::InconsistentRestore(format!(
                    "position {} was loaded as open but has status {:?}",
                    position.id, position.status
                )));
            }
            let key = (position.instrument_id, position.strategy_id.clone());
            if let Some(earlier_id) = seen_instrument_strategy_pairs.insert(key, position.id) {
                return Err(PortfolioError::InconsistentRestore(format!(
                    "multiple open positions found for instrument {:?} and strategy {:?}; \
                     at most one per (instrument, strategy) is allowed (conflicting ids: {} and {})",
                    position.instrument_id, position.strategy_id, earlier_id, position.id
                )));
            }
        }
        for position in &closed_positions {
            if position.status != PositionStatus::Closed {
                return Err(PortfolioError::InconsistentRestore(format!(
                    "position {} was loaded as closed but has status {:?}",
                    position.id, position.status
                )));
            }
        }

        info!(
            open_positions = open_positions.len(),
            closed_positions = closed_positions.len(),
            cash = %current_cash,
            "portfolio state restored"
        );

        Ok(Self {
            initial_cash,
            cash: current_cash,
            open_positions,
            closed_positions,
        })
    }

    pub fn cash(&self) -> Money {
        self.cash
    }

    pub fn open_positions(&self) -> &[Position] {
        &self.open_positions
    }

    pub fn closed_positions(&self) -> &[Position] {
        &self.closed_positions
    }

    /// A posição aberta de `strategy_id` (o "robô") em `instrument_id`, se
    /// houver — no máximo uma por par (ver ADR-20). Nunca retorna a
    /// posição de outro robô no mesmo instrumento.
    pub fn open_position_for(
        &self,
        instrument_id: InstrumentId,
        strategy_id: &StrategyId,
    ) -> Option<&Position> {
        self.open_positions
            .iter()
            .find(|p| p.instrument_id == instrument_id && &p.strategy_id == strategy_id)
    }

    /// Todas as posições abertas em `instrument_id`, de qualquer robô —
    /// usado para checar stop/take-profit por posição (cada robô tem seu
    /// próprio preço de entrada) e para exposição agregada por instrumento.
    pub fn open_positions_for_instrument(&self, instrument_id: InstrumentId) -> Vec<&Position> {
        self.open_positions
            .iter()
            .filter(|p| p.instrument_id == instrument_id)
            .collect()
    }

    /// Todas as posições abertas de `strategy_id`, em qualquer instrumento
    /// — PnL/exposição individuais de um robô partem daqui.
    pub fn open_positions_for_strategy(&self, strategy_id: &StrategyId) -> Vec<&Position> {
        self.open_positions
            .iter()
            .filter(|p| &p.strategy_id == strategy_id)
            .collect()
    }

    /// Exposição (notional de entrada) agregada de todos os robôs num
    /// único instrumento — o complemento "por instrumento" da exposição
    /// global já calculada por `exposure_ratio`/`equity`.
    pub fn exposure_for_instrument(&self, instrument_id: InstrumentId) -> Money {
        let reserved: Decimal = self
            .open_positions_for_instrument(instrument_id)
            .iter()
            .map(|p| p.entry_price * p.quantity)
            .sum();
        Money::new(reserved)
    }

    /// `spread_cost`/`slippage_cost` são registrados na `Position`
    /// resultante para atribuição posterior (veja `Position::spread_paid`),
    /// mas *não* são deduzidos separadamente do caixa aqui — eles já estão
    /// embutidos em `entry_price` (espera-se que o chamador passe um preço
    /// de execução já ajustado por spread/slippage), então subtraí-los
    /// novamente cobraria o trader em dobro. Apenas `fee` é um custo que
    /// não está refletido de outra forma em `entry_price`.
    #[allow(clippy::too_many_arguments)]
    pub fn open_position(
        &mut self,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        side: Side,
        quantity: Decimal,
        entry_price: Decimal,
        fee: Decimal,
        spread_cost: Decimal,
        slippage_cost: Decimal,
        opened_at: DateTime<Utc>,
    ) -> Position {
        let entry_notional = entry_price * quantity;
        self.cash = Money::new(self.cash.value() - entry_notional - fee);

        let position = Position {
            id: Uuid::new_v4(),
            instrument_id,
            strategy_id,
            side,
            quantity,
            entry_price,
            exit_price: None,
            opened_at,
            closed_at: None,
            status: PositionStatus::Open,
            realized_pnl_gross: None,
            realized_pnl_net: None,
            fees_paid: fee,
            spread_paid: spread_cost,
            slippage_paid: slippage_cost,
        };

        info!(
            instrument_id = ?position.instrument_id,
            side = ?side,
            quantity = %quantity,
            entry_price = %entry_price,
            cash_remaining = %self.cash,
            "position opened"
        );

        self.open_positions.push(position.clone());
        position
    }

    /// Vale a mesma ressalva de não contar em dobro que em `open_position`:
    /// `spread_cost`/`slippage_cost` são registrados na `Position`
    /// retornada para atribuição, mas não são deduzidos separadamente — já
    /// estão embutidos em `exit_price`. `realized_pnl_net` continua sendo
    /// exatamente `realized_pnl_gross - fees_paid`.
    #[allow(clippy::too_many_arguments)]
    pub fn close_position(
        &mut self,
        instrument_id: InstrumentId,
        strategy_id: &StrategyId,
        exit_price: Decimal,
        fee: Decimal,
        spread_cost: Decimal,
        slippage_cost: Decimal,
        closed_at: DateTime<Utc>,
    ) -> Result<Position, PortfolioError> {
        let idx = self
            .open_positions
            .iter()
            .position(|p| p.instrument_id == instrument_id && &p.strategy_id == strategy_id)
            .ok_or_else(|| PortfolioError::NoOpenPosition {
                instrument_id,
                strategy_id: strategy_id.clone(),
            })?;

        let mut position = self.open_positions.remove(idx);
        let entry_notional = position.entry_price * position.quantity;
        let pnl_gross = position.unrealized_pnl(exit_price);
        let pnl_net = pnl_gross - position.fees_paid - fee;

        self.cash = Money::new(self.cash.value() + entry_notional + pnl_gross - fee);

        position.exit_price = Some(exit_price);
        position.closed_at = Some(closed_at);
        position.status = PositionStatus::Closed;
        position.realized_pnl_gross = Some(pnl_gross);
        position.realized_pnl_net = Some(pnl_net);
        position.fees_paid += fee;
        position.spread_paid += spread_cost;
        position.slippage_paid += slippage_cost;

        info!(
            instrument_id = ?position.instrument_id,
            pnl_gross = %pnl_gross,
            pnl_net = %pnl_net,
            cash_after = %self.cash,
            "position closed"
        );

        self.closed_positions.push(position.clone());
        Ok(position)
    }

    /// Soma do P&L não realizado das posições abertas, usando `mark_prices`
    /// quando houver preço disponível para o instrumento e recorrendo ao
    /// preço de entrada (ou seja, P&L não realizado zero) caso contrário —
    /// uma posição para a qual o chamador não tem preço atualizado é
    /// reportada como neutra, em vez de causar panic ou erro.
    pub fn unrealized_pnl(&self, mark_prices: &HashMap<InstrumentId, Decimal>) -> Money {
        self.open_positions.iter().fold(Money::zero(), |acc, p| {
            let mark = mark_prices
                .get(&p.instrument_id)
                .copied()
                .unwrap_or(p.entry_price);
            acc + Money::new(p.unrealized_pnl(mark))
        })
    }

    pub fn realized_pnl_total(&self) -> Money {
        self.closed_positions.iter().fold(Money::zero(), |acc, p| {
            acc + p.realized_pnl_net.map(Money::new).unwrap_or_default()
        })
    }

    pub fn realized_pnl_today(&self, at: DateTime<Utc>) -> Money {
        self.closed_positions
            .iter()
            .filter(|p| p.closed_at.is_some_and(|c| is_same_utc_day(c, at)))
            .fold(Money::zero(), |acc, p| {
                acc + p.realized_pnl_net.map(Money::new).unwrap_or_default()
            })
    }

    /// Equity = caixa + colateral reservado pelas posições abertas + o P&L
    /// não realizado delas a `mark_prices`.
    pub fn equity(&self, mark_prices: &HashMap<InstrumentId, Decimal>) -> Money {
        let reserved: Decimal = self
            .open_positions
            .iter()
            .map(|p| p.entry_price * p.quantity)
            .sum();
        self.cash + Money::new(reserved) + self.unrealized_pnl(mark_prices)
    }

    /// Fração do equity atualmente alocada como colateral de posições
    /// abertas.
    pub fn exposure_ratio(&self, mark_prices: &HashMap<InstrumentId, Decimal>) -> Decimal {
        let equity = self.equity(mark_prices).value();
        if equity <= Decimal::ZERO {
            return Decimal::ZERO;
        }
        let reserved: Decimal = self
            .open_positions
            .iter()
            .map(|p| p.entry_price * p.quantity)
            .sum();
        reserved / equity
    }

    pub fn return_pct(&self, mark_prices: &HashMap<InstrumentId, Decimal>) -> Decimal {
        if self.initial_cash.value() <= Decimal::ZERO {
            return Decimal::ZERO;
        }
        (self.equity(mark_prices).value() - self.initial_cash.value()) / self.initial_cash.value()
    }

    pub fn snapshot(
        &self,
        mark_prices: &HashMap<InstrumentId, Decimal>,
        timestamp: DateTime<Utc>,
    ) -> PortfolioSnapshot {
        PortfolioSnapshot {
            timestamp,
            cash: self.cash.value(),
            equity: self.equity(mark_prices).value(),
            realized_pnl: self.realized_pnl_total().value(),
            unrealized_pnl: self.unrealized_pnl(mark_prices).value(),
            open_positions_count: self.open_positions.len() as u32,
            exposure_ratio: self.exposure_ratio(mark_prices),
            return_pct: self.return_pct(mark_prices),
            realized_pnl_today: self.realized_pnl_today(timestamp).value(),
        }
    }

    /// Uma visão somente leitura para o motor de risco. O equity aqui é
    /// aproximado com o colateral a preço de entrada (não com marcações
    /// atuais), para casar com a aproximação de exposição que
    /// `risk::PortfolioState` já documenta — o motor de risco nunca precisa
    /// de um equity totalmente marcado a mercado para avaliar um novo
    /// sinal.
    pub fn risk_snapshot(&self, at: DateTime<Utc>) -> risk::PortfolioState<'_> {
        let reserved: Decimal = self
            .open_positions
            .iter()
            .map(|p| p.entry_price * p.quantity)
            .sum();
        risk::PortfolioState {
            cash: self.cash,
            equity: self.cash + Money::new(reserved),
            open_positions: &self.open_positions,
            realized_pnl_today: self.realized_pnl_today(at),
        }
    }
}

fn is_same_utc_day(a: DateTime<Utc>, b: DateTime<Utc>) -> bool {
    a.year() == b.year() && a.ordinal() == b.ordinal()
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{InstrumentId, StrategyId};
    use rust_decimal_macros::dec;

    fn strategy() -> StrategyId {
        StrategyId::new("test").unwrap()
    }

    #[test]
    fn opening_position_debits_cash_by_notional_plus_fee() {
        let mut portfolio = PortfolioManager::new(Money::new(dec!(10000)));
        let instrument = InstrumentId::new();
        portfolio.open_position(
            instrument,
            strategy(),
            Side::Buy,
            dec!(1),
            dec!(100),
            dec!(0.5),
            dec!(0),
            dec!(0),
            Utc::now(),
        );
        // 10000 - (100*1) - 0.5 = 9899.5
        assert_eq!(portfolio.cash(), Money::new(dec!(9899.5)));
        assert_eq!(portfolio.open_positions().len(), 1);
    }

    #[test]
    fn closing_profitable_long_position_credits_cash() {
        let mut portfolio = PortfolioManager::new(Money::new(dec!(10000)));
        let instrument = InstrumentId::new();
        portfolio.open_position(
            instrument,
            strategy(),
            Side::Buy,
            dec!(1),
            dec!(100),
            dec!(0),
            dec!(0),
            dec!(0),
            Utc::now(),
        );
        let closed = portfolio
            .close_position(
                instrument,
                &strategy(),
                dec!(120),
                dec!(0),
                dec!(0),
                dec!(0),
                Utc::now(),
            )
            .unwrap();

        assert_eq!(closed.realized_pnl_gross, Some(dec!(20)));
        assert_eq!(closed.realized_pnl_net, Some(dec!(20)));
        // caixa devolvido: 100 de colateral + 20 de pnl = 10000 - 100 + 100 + 20 = 10020
        assert_eq!(portfolio.cash(), Money::new(dec!(10020)));
        assert!(portfolio.open_positions().is_empty());
        assert_eq!(portfolio.closed_positions().len(), 1);
    }

    #[test]
    fn closing_losing_short_position_reflects_loss() {
        let mut portfolio = PortfolioManager::new(Money::new(dec!(10000)));
        let instrument = InstrumentId::new();
        portfolio.open_position(
            instrument,
            strategy(),
            Side::Sell,
            dec!(1),
            dec!(100),
            dec!(0),
            dec!(0),
            dec!(0),
            Utc::now(),
        );
        // Preço subiu para 110 -> a posição short perde 10.
        let closed = portfolio
            .close_position(
                instrument,
                &strategy(),
                dec!(110),
                dec!(0),
                dec!(0),
                dec!(0),
                Utc::now(),
            )
            .unwrap();
        assert_eq!(closed.realized_pnl_gross, Some(dec!(-10)));
        assert_eq!(portfolio.cash(), Money::new(dec!(9990)));
    }

    #[test]
    fn close_position_without_open_returns_error() {
        let mut portfolio = PortfolioManager::new(Money::new(dec!(10000)));
        let result = portfolio.close_position(
            InstrumentId::new(),
            &strategy(),
            dec!(100),
            dec!(0),
            dec!(0),
            dec!(0),
            Utc::now(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn equity_reflects_unrealized_pnl_at_mark_price() {
        let mut portfolio = PortfolioManager::new(Money::new(dec!(10000)));
        let instrument = InstrumentId::new();
        portfolio.open_position(
            instrument,
            strategy(),
            Side::Buy,
            dec!(2),
            dec!(100),
            dec!(0),
            dec!(0),
            dec!(0),
            Utc::now(),
        );
        let mut marks = HashMap::new();
        marks.insert(instrument, dec!(110));
        // caixa 9800 + reservado 200 + não realizado (110-100)*2=20 => 10020
        assert_eq!(portfolio.equity(&marks), Money::new(dec!(10020)));
        assert_eq!(portfolio.return_pct(&marks), dec!(0.002));
    }

    #[test]
    fn risk_snapshot_exposes_open_positions_and_cash() {
        let mut portfolio = PortfolioManager::new(Money::new(dec!(10000)));
        let instrument = InstrumentId::new();
        portfolio.open_position(
            instrument,
            strategy(),
            Side::Buy,
            dec!(1),
            dec!(100),
            dec!(0),
            dec!(0),
            dec!(0),
            Utc::now(),
        );
        let snapshot = portfolio.risk_snapshot(Utc::now());
        assert_eq!(snapshot.cash, Money::new(dec!(9900)));
        assert_eq!(snapshot.open_position_count(), 1);
    }

    /// Constrói uma `Position` "carregada do Postgres" com os campos que
    /// cada teste de `restore` precisa controlar, sem passar pelo fluxo
    /// normal de `open_position`/`close_position` (que gera seu próprio
    /// `id`/timestamps).
    #[allow(clippy::too_many_arguments)]
    fn loaded_position(
        instrument_id: InstrumentId,
        status: PositionStatus,
        realized_pnl_net: Option<Decimal>,
        closed_at: Option<DateTime<Utc>>,
    ) -> Position {
        Position {
            id: Uuid::new_v4(),
            instrument_id,
            strategy_id: strategy(),
            side: Side::Buy,
            quantity: dec!(1),
            entry_price: dec!(100),
            exit_price: if status == PositionStatus::Closed {
                Some(dec!(110))
            } else {
                None
            },
            opened_at: Utc::now(),
            closed_at,
            status,
            realized_pnl_gross: realized_pnl_net,
            realized_pnl_net,
            fees_paid: dec!(1),
            spread_paid: dec!(0.5),
            slippage_paid: dec!(0.5),
        }
    }

    #[test]
    fn restore_reconstructs_cash_and_open_positions() {
        let instrument = InstrumentId::new();
        let open = loaded_position(instrument, PositionStatus::Open, None, None);
        let open_id = open.id;

        let portfolio = PortfolioManager::restore(
            Money::new(dec!(10000)),
            Money::new(dec!(9899.5)),
            vec![open],
            vec![],
        )
        .unwrap();

        assert_eq!(portfolio.cash(), Money::new(dec!(9899.5)));
        assert_eq!(portfolio.open_positions().len(), 1);
        assert_eq!(portfolio.open_positions()[0].id, open_id);
        assert_eq!(
            portfolio
                .open_position_for(instrument, &strategy())
                .map(|p| p.id),
            Some(open_id)
        );
    }

    #[test]
    fn restore_preserves_realized_pnl_from_closed_positions() {
        let closed = loaded_position(
            InstrumentId::new(),
            PositionStatus::Closed,
            Some(dec!(50)),
            Some(Utc::now()),
        );

        let portfolio = PortfolioManager::restore(
            Money::new(dec!(10000)),
            Money::new(dec!(10050)),
            vec![],
            vec![closed],
        )
        .unwrap();

        // O P&L realizado histórico não é perdido no restart.
        assert_eq!(portfolio.realized_pnl_total(), Money::new(dec!(50)));
        assert_eq!(portfolio.closed_positions().len(), 1);
    }

    #[test]
    fn restore_rejects_duplicate_open_positions_for_same_instrument_and_strategy() {
        let instrument = InstrumentId::new();
        // Mesmo instrumento E mesma strategy_id (`loaded_position` usa
        // `strategy()` para as duas) — é essa combinação, não o
        // instrumento sozinho, que `restore` proíbe duplicar (ADR-20).
        let first = loaded_position(instrument, PositionStatus::Open, None, None);
        let second = loaded_position(instrument, PositionStatus::Open, None, None);

        let result = PortfolioManager::restore(
            Money::new(dec!(10000)),
            Money::new(dec!(9000)),
            vec![first, second],
            vec![],
        );

        assert!(matches!(
            result,
            Err(PortfolioError::InconsistentRestore(_))
        ));
    }

    /// Requisito 1 (multi-robô) ao nível de `restore`: duas posições
    /// abertas no mesmo instrumento, mas de robôs (strategy_id)
    /// diferentes, não são uma inconsistência — `restore` só rejeita
    /// duplicidade no par (instrumento, estratégia), nunca no instrumento
    /// sozinho.
    #[test]
    fn restore_accepts_two_open_positions_for_same_instrument_from_different_strategies() {
        let instrument = InstrumentId::new();
        let mut robot_a = loaded_position(instrument, PositionStatus::Open, None, None);
        robot_a.strategy_id = StrategyId::new("robot-a").unwrap();
        let mut robot_b = loaded_position(instrument, PositionStatus::Open, None, None);
        robot_b.strategy_id = StrategyId::new("robot-b").unwrap();

        let portfolio = PortfolioManager::restore(
            Money::new(dec!(10000)),
            Money::new(dec!(9000)),
            vec![robot_a, robot_b],
            vec![],
        )
        .unwrap();

        assert_eq!(portfolio.open_positions().len(), 2);
        assert!(portfolio
            .open_position_for(instrument, &StrategyId::new("robot-a").unwrap())
            .is_some());
        assert!(portfolio
            .open_position_for(instrument, &StrategyId::new("robot-b").unwrap())
            .is_some());
    }

    #[test]
    fn restore_rejects_open_position_list_containing_a_closed_position() {
        let wrongly_placed = loaded_position(
            InstrumentId::new(),
            PositionStatus::Closed,
            Some(dec!(10)),
            Some(Utc::now()),
        );

        let result = PortfolioManager::restore(
            Money::new(dec!(10000)),
            Money::new(dec!(9000)),
            vec![wrongly_placed],
            vec![],
        );

        assert!(matches!(
            result,
            Err(PortfolioError::InconsistentRestore(_))
        ));
    }

    #[test]
    fn restore_rejects_closed_position_list_containing_an_open_position() {
        let wrongly_placed = loaded_position(InstrumentId::new(), PositionStatus::Open, None, None);

        let result = PortfolioManager::restore(
            Money::new(dec!(10000)),
            Money::new(dec!(9000)),
            vec![],
            vec![wrongly_placed],
        );

        assert!(matches!(
            result,
            Err(PortfolioError::InconsistentRestore(_))
        ));
    }

    #[test]
    fn restored_open_position_is_visible_via_risk_snapshot() {
        let instrument = InstrumentId::new();
        let open = loaded_position(instrument, PositionStatus::Open, None, None);

        let portfolio = PortfolioManager::restore(
            Money::new(dec!(10000)),
            Money::new(dec!(9899.5)),
            vec![open],
            vec![],
        )
        .unwrap();

        // É exatamente isso que `risk::RiskEngine::evaluate` consulta para
        // decidir se já existe posição aberta no instrumento — a posição
        // restaurada precisa aparecer aqui, não só em `open_positions()`.
        let snapshot = portfolio.risk_snapshot(Utc::now());
        assert!(snapshot
            .open_position_for(instrument, &strategy())
            .is_some());
        assert_eq!(snapshot.open_position_count(), 1);
    }

    /// Reconciliação contábil do modelo atual: `equity` precisa bater
    /// exatamente com `capital_inicial + P&L_realizado + P&L_não_realizado`
    /// — MENOS as fees pagas na abertura das posições ainda em aberto, que
    /// já reduziram `cash`/`equity` mas não aparecem em `unrealized_pnl`
    /// (que é puramente `(mark - entry) * quantity`, sem custos — ver
    /// `domain::Position::unrealized_pnl`). Esse ajuste não é uma
    /// aproximação: dado que `close_position` devolve `entry_notional +
    /// pnl_gross - fee` ao caixa (então o efeito líquido de um ciclo
    /// completo abrir+fechar em `cash` é exatamente `pnl_net`), a álgebra
    /// da modelagem atual implica essa igualdade exata, não só
    /// aproximada — e é isso que este teste prova, cobrindo posições Buy e
    /// Sell, fechadas e ainda abertas, com fees em todas as pontas.
    #[test]
    fn equity_reconciles_with_initial_cash_plus_realized_plus_unrealized_pnl() {
        let initial_cash = Money::new(dec!(10000));
        let mut portfolio = PortfolioManager::new(initial_cash);

        // Ciclo 1: compra lucrativa, fechada.
        let inst_a = InstrumentId::new();
        portfolio.open_position(
            inst_a,
            strategy(),
            Side::Buy,
            dec!(2),
            dec!(100),
            dec!(1),
            dec!(0),
            dec!(0),
            Utc::now(),
        );
        portfolio
            .close_position(
                inst_a,
                &strategy(),
                dec!(110),
                dec!(1),
                dec!(0),
                dec!(0),
                Utc::now(),
            )
            .unwrap();

        // Ciclo 2: venda lucrativa (short fechado com queda de preço), fechada.
        let inst_b = InstrumentId::new();
        portfolio.open_position(
            inst_b,
            strategy(),
            Side::Sell,
            dec!(1),
            dec!(50),
            dec!(0.5),
            dec!(0),
            dec!(0),
            Utc::now(),
        );
        portfolio
            .close_position(
                inst_b,
                &strategy(),
                dec!(45),
                dec!(0.5),
                dec!(0),
                dec!(0),
                Utc::now(),
            )
            .unwrap();

        // Duas posições ficam abertas até o fim, cada uma com sua própria
        // fee de entrada — é exatamente esse custo que o ajuste do teste
        // precisa descontar.
        let inst_c = InstrumentId::new();
        portfolio.open_position(
            inst_c,
            strategy(),
            Side::Buy,
            dec!(1),
            dec!(200),
            dec!(2),
            dec!(1),
            dec!(1),
            Utc::now(),
        );
        let inst_d = InstrumentId::new();
        portfolio.open_position(
            inst_d,
            strategy(),
            Side::Sell,
            dec!(3),
            dec!(30),
            dec!(0.9),
            dec!(0),
            dec!(0),
            Utc::now(),
        );

        let mut mark_prices = HashMap::new();
        mark_prices.insert(inst_c, dec!(210));
        mark_prices.insert(inst_d, dec!(28));

        let realized_pnl = portfolio.realized_pnl_total();
        let unrealized_pnl = portfolio.unrealized_pnl(&mark_prices);
        let open_entry_fees: Decimal = portfolio.open_positions().iter().map(|p| p.fees_paid).sum();
        let equity = portfolio.equity(&mark_prices);

        assert_eq!(
            realized_pnl,
            Money::new(dec!(22)),
            "18 (ciclo 1) + 4 (ciclo 2)"
        );
        assert_eq!(unrealized_pnl, Money::new(dec!(16)), "10 (C) + 6 (D)");
        assert_eq!(open_entry_fees, dec!(2.9), "fee de C (2) + fee de D (0.9)");
        assert_eq!(
            equity,
            initial_cash + realized_pnl + unrealized_pnl - Money::new(open_entry_fees),
            "equity deve reconciliar exatamente com capital inicial + P&L realizado + P&L não \
             realizado, descontadas as fees de entrada das posições ainda abertas"
        );
        assert_eq!(equity, Money::new(dec!(10035.1)));
    }

    fn robot(name: &str) -> StrategyId {
        StrategyId::new(name).unwrap()
    }

    /// Requisito 1: dois robôs abrem posições simultâneas no mesmo
    /// instrumento — `open_position` nunca rejeitou por instrumento (só o
    /// Risk Engine decide isso, ver `risk::engine`), então isto já
    /// funcionava; o que confirma aqui é que as duas posições continuam
    /// individualmente enderaçáveis por `(instrument, strategy)` depois.
    #[test]
    fn two_robots_can_open_simultaneous_positions_on_the_same_instrument() {
        let mut portfolio = PortfolioManager::new(Money::new(dec!(100000)));
        let instrument = InstrumentId::new();

        portfolio.open_position(
            instrument,
            robot("robot-a"),
            Side::Buy,
            dec!(1),
            dec!(100),
            dec!(0),
            dec!(0),
            dec!(0),
            Utc::now(),
        );
        portfolio.open_position(
            instrument,
            robot("robot-b"),
            Side::Sell,
            dec!(1),
            dec!(100),
            dec!(0),
            dec!(0),
            dec!(0),
            Utc::now(),
        );

        assert_eq!(portfolio.open_positions().len(), 2);
        assert_eq!(
            portfolio
                .open_position_for(instrument, &robot("robot-a"))
                .unwrap()
                .side,
            Side::Buy
        );
        assert_eq!(
            portfolio
                .open_position_for(instrument, &robot("robot-b"))
                .unwrap()
                .side,
            Side::Sell
        );
    }

    /// Requisitos 2/3: a posição de um robô nunca é confundida com a de
    /// outro, e fechar a de A não fecha (nem afeta) a de B.
    #[test]
    fn closing_one_robots_position_does_not_affect_another_robots_position() {
        let mut portfolio = PortfolioManager::new(Money::new(dec!(100000)));
        let instrument = InstrumentId::new();

        portfolio.open_position(
            instrument,
            robot("robot-a"),
            Side::Buy,
            dec!(1),
            dec!(100),
            dec!(0),
            dec!(0),
            dec!(0),
            Utc::now(),
        );
        let opened_b = portfolio.open_position(
            instrument,
            robot("robot-b"),
            Side::Buy,
            dec!(2),
            dec!(200),
            dec!(0),
            dec!(0),
            dec!(0),
            Utc::now(),
        );

        portfolio
            .close_position(
                instrument,
                &robot("robot-a"),
                dec!(110),
                dec!(0),
                dec!(0),
                dec!(0),
                Utc::now(),
            )
            .unwrap();

        // A de A sumiu das abertas; a de B permanece intocada (mesmo id,
        // mesma quantidade/entry_price de quando foi aberta).
        assert!(portfolio
            .open_position_for(instrument, &robot("robot-a"))
            .is_none());
        let still_open_b = portfolio
            .open_position_for(instrument, &robot("robot-b"))
            .expect("robot-b's position must still be open");
        assert_eq!(still_open_b.id, opened_b.id);
        assert_eq!(still_open_b.quantity, dec!(2));
        assert_eq!(still_open_b.entry_price, dec!(200));
        assert_eq!(portfolio.open_positions().len(), 1);
    }

    /// Requisito 6: PnL individual continua separado por robô — fechar a
    /// posição lucrativa de A não mistura seu `realized_pnl_net` com o
    /// P&L (ainda não realizado) de B.
    #[test]
    fn individual_realized_pnl_stays_separated_per_robot() {
        let mut portfolio = PortfolioManager::new(Money::new(dec!(100000)));
        let instrument = InstrumentId::new();

        portfolio.open_position(
            instrument,
            robot("robot-a"),
            Side::Buy,
            dec!(1),
            dec!(100),
            dec!(0),
            dec!(0),
            dec!(0),
            Utc::now(),
        );
        portfolio.open_position(
            instrument,
            robot("robot-b"),
            Side::Buy,
            dec!(1),
            dec!(100),
            dec!(0),
            dec!(0),
            dec!(0),
            Utc::now(),
        );

        let closed_a = portfolio
            .close_position(
                instrument,
                &robot("robot-a"),
                dec!(150),
                dec!(0),
                dec!(0),
                dec!(0),
                Utc::now(),
            )
            .unwrap();

        assert_eq!(closed_a.strategy_id, robot("robot-a"));
        assert_eq!(closed_a.realized_pnl_net, Some(dec!(50)));
        assert_eq!(
            portfolio.closed_positions().len(),
            1,
            "only robot-a's position was closed"
        );
        assert_eq!(
            portfolio.realized_pnl_total(),
            Money::new(dec!(50)),
            "robot-b's still-open position must not contribute to realized PnL"
        );
        // A posição de B continua aberta e sem nenhum PnL realizado.
        let open_b = portfolio
            .open_position_for(instrument, &robot("robot-b"))
            .unwrap();
        assert_eq!(open_b.realized_pnl_net, None);
    }

    /// Requisito 7: exposição agregada por instrumento soma as posições
    /// de todos os robôs; `open_positions_for_strategy` isola a de um só.
    #[test]
    fn exposure_and_positions_aggregate_correctly_across_robots() {
        let mut portfolio = PortfolioManager::new(Money::new(dec!(100000)));
        let instrument = InstrumentId::new();

        portfolio.open_position(
            instrument,
            robot("robot-a"),
            Side::Buy,
            dec!(1),
            dec!(100),
            dec!(0),
            dec!(0),
            dec!(0),
            Utc::now(),
        );
        portfolio.open_position(
            instrument,
            robot("robot-b"),
            Side::Buy,
            dec!(2),
            dec!(50),
            dec!(0),
            dec!(0),
            dec!(0),
            Utc::now(),
        );

        // 1*100 + 2*50 = 200, independentemente de quantos robôs
        // contribuíram para o total.
        assert_eq!(
            portfolio.exposure_for_instrument(instrument),
            Money::new(dec!(200))
        );
        assert_eq!(portfolio.open_positions_for_instrument(instrument).len(), 2);
        assert_eq!(
            portfolio
                .open_positions_for_strategy(&robot("robot-a"))
                .len(),
            1
        );
        assert_eq!(
            portfolio.open_positions_for_strategy(&robot("robot-a"))[0].entry_price,
            dec!(100)
        );
    }
}
