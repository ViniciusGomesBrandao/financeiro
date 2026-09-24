use chrono::Utc;
use domain::{
    notional, Instrument, Money, OrderRequest, OrderType, Position, Price, Quantity, Side, Signal,
    SignalDirection,
};
use rust_decimal::Decimal;
use tracing::{info, warn};

use crate::config::RiskConfig;
use crate::decision::{ExitReason, RejectionReason, RiskDecision};
use crate::portfolio_state::PortfolioState;

/// Aplica os limites de `RiskConfig` aos sinais das estratégias e às
/// posições abertas.
///
/// O motor de risco nunca conversa com um broker nem com o banco de dados,
/// e nunca altera o estado do portfólio — ele apenas lê um snapshot de
/// `PortfolioState` e retorna uma decisão. O dimensionamento de sinal para
/// ordem aqui é deliberadamente simples (notional fixo por trade); veja
/// `RiskConfig::order_notional`.
pub struct RiskEngine {
    config: RiskConfig,
}

impl RiskEngine {
    pub fn new(config: RiskConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &RiskConfig {
        &self.config
    }

    /// Avalia um sinal contra os limites de risco atuais e o estado do
    /// portfólio, produzindo um `OrderRequest` ou uma rejeição explícita.
    pub fn evaluate(
        &self,
        signal: &Signal,
        instrument: &Instrument,
        current_price: Price,
        portfolio: &PortfolioState,
    ) -> RiskDecision {
        let existing = portfolio.open_position_for(instrument.id, &signal.strategy_id);

        // Contas spot não podem abrir um short: não há mecanismo de
        // empréstimo/margem para vender um ativo que o portfólio não
        // possui. Um sinal `Short` só é acionável como saída de uma posição
        // long existente (economicamente equivalente a `Flat` nesse caso);
        // sem posição aberta, ele nunca pode gerar uma ordem.
        let decision = match signal.direction {
            SignalDirection::Flat => self.evaluate_flat(signal, existing, current_price),
            SignalDirection::Short if existing.is_some() => {
                self.evaluate_flat(signal, existing, current_price)
            }
            SignalDirection::Short => {
                RiskDecision::Rejected(RejectionReason::ShortSellingNotSupported)
            }
            SignalDirection::Long => {
                self.evaluate_entry(signal, instrument, current_price, portfolio, existing)
            }
        };

        match &decision {
            RiskDecision::Approved(order) => {
                info!(
                    strategy_id = %signal.strategy_id,
                    instrument = %instrument.symbol,
                    side = ?order.side,
                    quantity = %order.quantity,
                    "risk decision: approved"
                );
            }
            RiskDecision::Rejected(reason) => {
                warn!(
                    strategy_id = %signal.strategy_id,
                    instrument = %instrument.symbol,
                    reason = %reason,
                    "risk decision: rejected"
                );
            }
        }
        decision
    }

    fn evaluate_flat(
        &self,
        signal: &Signal,
        existing: Option<&Position>,
        _current_price: Price,
    ) -> RiskDecision {
        let Some(position) = existing else {
            return RiskDecision::Rejected(RejectionReason::NothingToFlatten);
        };

        RiskDecision::Approved(OrderRequest {
            instrument_id: signal.instrument_id,
            strategy_id: signal.strategy_id.clone(),
            signal_id: Some(signal.id),
            side: position.side.opposite(),
            order_type: OrderType::Market,
            quantity: position.quantity,
            limit_price: None,
            requested_at: signal.timestamp,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_entry(
        &self,
        signal: &Signal,
        instrument: &Instrument,
        current_price: Price,
        portfolio: &PortfolioState,
        existing: Option<&Position>,
    ) -> RiskDecision {
        if existing.is_some() {
            return RiskDecision::Rejected(RejectionReason::PositionAlreadyOpen);
        }

        if portfolio.open_position_count() >= self.config.max_open_positions {
            return RiskDecision::Rejected(RejectionReason::MaxOpenPositionsReached {
                current: portfolio.open_position_count(),
                max: self.config.max_open_positions,
            });
        }

        if portfolio.realized_pnl_today.is_negative()
            && portfolio.realized_pnl_today.value().abs() >= self.config.max_daily_loss.value()
        {
            return RiskDecision::Rejected(RejectionReason::DailyLossLimitBreached {
                realized_today: portfolio.realized_pnl_today,
                limit: self.config.max_daily_loss,
            });
        }

        let requested_notional = self
            .config
            .order_notional
            .value()
            .min(self.config.max_position_notional.value());
        if requested_notional <= Decimal::ZERO {
            return RiskDecision::Rejected(RejectionReason::PositionSizeExceeded {
                requested: self.config.order_notional,
                max: self.config.max_position_notional,
            });
        }

        if Money::new(requested_notional) < instrument_min_notional(instrument) {
            return RiskDecision::Rejected(RejectionReason::BelowExchangeMinimum {
                requested: Money::new(requested_notional),
                minimum: instrument_min_notional(instrument),
            });
        }

        // Preço que de fato será usado na execução (arredondado ao
        // tick_size do instrumento), calculado uma única vez e reutilizado
        // em todo o dimensionamento abaixo — evita divergência entre o
        // preço usado para estimar a quantidade e o preço usado para
        // validar o notional final.
        let execution_price_decimal = instrument.round_price_to_tick(current_price.value());
        let Ok(execution_price) = Price::new(execution_price_decimal) else {
            return RiskDecision::Rejected(RejectionReason::BelowExchangeMinimum {
                requested: Money::new(requested_notional),
                minimum: instrument_min_notional(instrument),
            });
        };

        let raw_quantity = requested_notional / execution_price.value();
        // Quantiza para o lot_size do instrumento antes de qualquer outra
        // checagem — todo número calculado a seguir (notional, exposição,
        // caixa) deve refletir o que de fato pode ser executado, não o
        // ideal pré-arredondamento. Sempre arredonda para baixo, então a
        // ordem resultante nunca excede o notional/caixa aprovado.
        // Centralizado em `Instrument` para que `risk` e `execution`
        // compartilhem uma única regra de arredondamento.
        let quantity_decimal = instrument.round_quantity_down_to_lot(raw_quantity);
        let Ok(quantity) = Quantity::new(quantity_decimal) else {
            return RiskDecision::Rejected(RejectionReason::BelowExchangeMinimum {
                requested: Money::new(requested_notional),
                minimum: instrument_min_notional(instrument),
            });
        };
        if quantity.value() < instrument.min_quantity {
            return RiskDecision::Rejected(RejectionReason::BelowExchangeMinimum {
                requested: Money::new(requested_notional),
                minimum: instrument_min_notional(instrument),
            });
        }

        let order_notional = notional(execution_price, quantity);
        // Revalida o mínimo de notional aqui, não só antes do
        // arredondamento: o arredondamento de quantity para baixo (lot_size)
        // pode fazer o notional real cair abaixo do mínimo mesmo quando o
        // notional pré-arredondamento estava acima. Sem esta checagem, uma
        // ordem executável na prática abaixo do mínimo da exchange passaria
        // pela validação.
        if order_notional < instrument_min_notional(instrument) {
            return RiskDecision::Rejected(RejectionReason::BelowExchangeMinimum {
                requested: order_notional,
                minimum: instrument_min_notional(instrument),
            });
        }
        if order_notional.value() > self.config.max_position_notional.value() {
            return RiskDecision::Rejected(RejectionReason::PositionSizeExceeded {
                requested: order_notional,
                max: self.config.max_position_notional,
            });
        }

        let projected_exposure = portfolio.total_exposure() + order_notional;
        if projected_exposure.value() > self.config.max_total_exposure.value() {
            return RiskDecision::Rejected(RejectionReason::ExposureLimitExceeded {
                projected: projected_exposure,
                max: self.config.max_total_exposure,
            });
        }

        if order_notional.value() > portfolio.cash.value() {
            return RiskDecision::Rejected(RejectionReason::InsufficientBalance {
                required: order_notional,
                available: portfolio.cash,
            });
        }

        debug_assert!(
            matches!(signal.direction, SignalDirection::Long),
            "evaluate_entry is only ever called for Long signals; Short/Flat are routed \
             to evaluate_flat or rejected before reaching here (see evaluate())"
        );

        RiskDecision::Approved(OrderRequest {
            instrument_id: signal.instrument_id,
            strategy_id: signal.strategy_id.clone(),
            signal_id: Some(signal.id),
            side: Side::Buy,
            order_type: OrderType::Market,
            quantity: quantity.value(),
            limit_price: None,
            requested_at: signal.timestamp,
        })
    }

    /// Verifica uma posição aberta contra os limiares configurados de stop
    /// loss / take profit no `mark_price`. Retorna `None` se nenhum dos
    /// dois está configurado ou se nenhum foi rompido. Espera-se que os
    /// chamadores (o condutor ao vivo/de backtest) invoquem isto para cada
    /// posição aberta a cada nova atualização de preço e encaminhem um
    /// resultado `Some` para uma ordem de zeragem.
    pub fn check_exit(&self, position: &Position, mark_price: Decimal) -> Option<ExitReason> {
        let pnl_pct = match position.side {
            Side::Buy => (mark_price - position.entry_price) / position.entry_price,
            Side::Sell => (position.entry_price - mark_price) / position.entry_price,
        };

        if let Some(stop_loss_pct) = self.config.stop_loss_pct {
            if pnl_pct <= -stop_loss_pct {
                return Some(ExitReason::StopLoss);
            }
        }
        if let Some(take_profit_pct) = self.config.take_profit_pct {
            if pnl_pct >= take_profit_pct {
                return Some(ExitReason::TakeProfit);
            }
        }
        None
    }

    /// Constrói o `OrderRequest` de zeragem para uma saída motivada por
    /// risco (stop loss / take profit), em contraste com um sinal `Flat`
    /// originado da estratégia — não há `Signal` por trás desta ordem.
    pub fn build_exit_order(&self, position: &Position, at: chrono::DateTime<Utc>) -> OrderRequest {
        OrderRequest {
            instrument_id: position.instrument_id,
            strategy_id: position.strategy_id.clone(),
            signal_id: None,
            side: position.side.opposite(),
            order_type: OrderType::Market,
            quantity: position.quantity,
            limit_price: None,
            requested_at: at,
        }
    }
}

fn instrument_min_notional(instrument: &Instrument) -> Money {
    Money::new(instrument.min_notional)
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{Asset, AssetClass, Exchange, MarketType, PositionStatus, StrategyId, Symbol};
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn instrument() -> Instrument {
        let base = Asset::new("BTC").unwrap();
        let quote = Asset::new("USDT").unwrap();
        Instrument::new(
            Symbol::from_pair(&base, &quote),
            base,
            quote,
            AssetClass::Crypto,
            Exchange::Binance,
            MarketType::Spot,
            dec!(0.01),
            dec!(0.0001),
            dec!(0.0001),
            dec!(10),
        )
    }

    fn base_config() -> RiskConfig {
        RiskConfig {
            order_notional: Money::new(dec!(1000)),
            max_position_notional: Money::new(dec!(1000)),
            max_total_exposure: Money::new(dec!(5000)),
            max_open_positions: 5,
            stop_loss_pct: Some(dec!(0.05)),
            take_profit_pct: Some(dec!(0.10)),
            max_daily_loss: Money::new(dec!(500)),
        }
    }

    fn long_signal(instrument_id: domain::InstrumentId) -> Signal {
        Signal::new(
            StrategyId::new("test").unwrap(),
            instrument_id,
            SignalDirection::Long,
            0.8,
            Utc::now(),
            None,
            None,
            serde_json::Value::Null,
        )
        .unwrap()
    }

    fn short_signal(instrument_id: domain::InstrumentId) -> Signal {
        Signal::new(
            StrategyId::new("test").unwrap(),
            instrument_id,
            SignalDirection::Short,
            0.8,
            Utc::now(),
            None,
            None,
            serde_json::Value::Null,
        )
        .unwrap()
    }

    fn empty_portfolio() -> PortfolioState<'static> {
        PortfolioState {
            cash: Money::new(dec!(10000)),
            equity: Money::new(dec!(10000)),
            open_positions: &[],
            realized_pnl_today: Money::zero(),
        }
    }

    #[test]
    fn approves_valid_entry_within_limits() {
        let engine = RiskEngine::new(base_config());
        let instrument = instrument();
        let signal = long_signal(instrument.id);
        let portfolio = empty_portfolio();
        let price = Price::new(dec!(50000)).unwrap();

        let decision = engine.evaluate(&signal, &instrument, price, &portfolio);
        assert!(decision.is_approved());
        if let RiskDecision::Approved(order) = decision {
            assert_eq!(order.side, Side::Buy);
            assert_eq!(order.quantity, dec!(0.02)); // 1000 / 50000
        }
    }

    #[test]
    fn quantity_is_rounded_down_to_lot_size() {
        let engine = RiskEngine::new(base_config());
        let instrument = instrument(); // lot_size = 0.0001
        let signal = long_signal(instrument.id);
        let portfolio = empty_portfolio();
        // 1000 / 33333 = 0.0300003000030000..., que não é um múltiplo
        // exato de 0.0001 e precisa ser arredondado *para baixo* até
        // 0.0300, nunca para cima (arredondar para cima poderia exceder o
        // notional/caixa aprovado).
        let price = Price::new(dec!(33333)).unwrap();

        let decision = engine.evaluate(&signal, &instrument, price, &portfolio);
        assert!(decision.is_approved());
        if let RiskDecision::Approved(order) = decision {
            assert_eq!(order.quantity, dec!(0.0300));
        }
    }

    #[test]
    fn rejects_when_lot_size_rounds_quantity_to_zero() {
        let mut config = base_config();
        config.order_notional = Money::new(dec!(100));
        config.max_position_notional = Money::new(dec!(100));
        let engine = RiskEngine::new(config);
        let mut instrument = instrument();
        instrument.lot_size = dec!(1); // apenas unidades inteiras
        instrument.min_notional = Decimal::ZERO; // isola o caminho do lot_size
        let signal = long_signal(instrument.id);
        let portfolio = empty_portfolio();
        // 100 / 50000 = 0.002, que truncado dá 0 unidades inteiras.
        let price = Price::new(dec!(50000)).unwrap();

        let decision = engine.evaluate(&signal, &instrument, price, &portfolio);
        assert!(matches!(
            decision,
            RiskDecision::Rejected(RejectionReason::BelowExchangeMinimum { .. })
        ));
    }

    #[test]
    fn rejects_when_rounded_notional_falls_below_minimum_after_lot_rounding() {
        // O notional solicitado (100) passa na checagem inicial (>= 100 =
        // min_notional), mas ao arredondar a quantidade para baixo pelo
        // lot_size, o notional realmente executável cai para 60 — abaixo
        // do mínimo. Sem a revalidação pós-arredondamento, esta ordem
        // seria aprovada incorretamente.
        let mut config = base_config();
        config.order_notional = Money::new(dec!(100));
        config.max_position_notional = Money::new(dec!(100));
        let engine = RiskEngine::new(config);
        let mut instrument = instrument();
        instrument.lot_size = dec!(2);
        instrument.min_notional = dec!(100);
        instrument.min_quantity = dec!(0.0001);
        let signal = long_signal(instrument.id);
        let portfolio = empty_portfolio();
        // raw_quantity = 100 / 30 = 3.333..., arredondado para baixo em
        // múltiplos de 2 -> 2. notional real = 2 * 30 = 60 < 100.
        let price = Price::new(dec!(30)).unwrap();

        let decision = engine.evaluate(&signal, &instrument, price, &portfolio);
        assert_eq!(
            decision,
            RiskDecision::Rejected(RejectionReason::BelowExchangeMinimum {
                requested: Money::new(dec!(60)),
                minimum: Money::new(dec!(100)),
            })
        );
    }

    #[test]
    fn approves_when_rounded_notional_still_clears_minimum() {
        // Mesmo cenário de arredondamento, mas com min_notional baixo o
        // suficiente para que o notional pós-arredondamento (60) ainda
        // seja aprovado — confirma que a nova checagem não rejeita ordens
        // válidas.
        let mut config = base_config();
        config.order_notional = Money::new(dec!(100));
        config.max_position_notional = Money::new(dec!(100));
        let engine = RiskEngine::new(config);
        let mut instrument = instrument();
        instrument.lot_size = dec!(2);
        instrument.min_notional = dec!(50);
        instrument.min_quantity = dec!(0.0001);
        let signal = long_signal(instrument.id);
        let portfolio = empty_portfolio();
        let price = Price::new(dec!(30)).unwrap();

        let decision = engine.evaluate(&signal, &instrument, price, &portfolio);
        assert!(decision.is_approved());
        if let RiskDecision::Approved(order) = decision {
            assert_eq!(order.quantity, dec!(2));
        }
    }

    #[test]
    fn rejects_when_insufficient_balance() {
        let engine = RiskEngine::new(base_config());
        let instrument = instrument();
        let signal = long_signal(instrument.id);
        let portfolio = PortfolioState {
            cash: Money::new(dec!(100)),
            equity: Money::new(dec!(100)),
            open_positions: &[],
            realized_pnl_today: Money::zero(),
        };
        let price = Price::new(dec!(50000)).unwrap();

        let decision = engine.evaluate(&signal, &instrument, price, &portfolio);
        assert_eq!(
            decision,
            RiskDecision::Rejected(RejectionReason::InsufficientBalance {
                required: Money::new(dec!(1000)),
                available: Money::new(dec!(100)),
            })
        );
    }

    #[test]
    fn rejects_when_max_open_positions_reached() {
        let mut config = base_config();
        config.max_open_positions = 0;
        let engine = RiskEngine::new(config);
        let instrument = instrument();
        let signal = long_signal(instrument.id);
        let portfolio = empty_portfolio();
        let price = Price::new(dec!(50000)).unwrap();

        let decision = engine.evaluate(&signal, &instrument, price, &portfolio);
        assert_eq!(
            decision,
            RiskDecision::Rejected(RejectionReason::MaxOpenPositionsReached { current: 0, max: 0 })
        );
    }

    #[test]
    fn rejects_when_daily_loss_limit_breached() {
        let engine = RiskEngine::new(base_config());
        let instrument = instrument();
        let signal = long_signal(instrument.id);
        let portfolio = PortfolioState {
            cash: Money::new(dec!(10000)),
            equity: Money::new(dec!(10000)),
            open_positions: &[],
            realized_pnl_today: Money::new(dec!(-500)),
        };
        let price = Price::new(dec!(50000)).unwrap();

        let decision = engine.evaluate(&signal, &instrument, price, &portfolio);
        assert!(matches!(
            decision,
            RiskDecision::Rejected(RejectionReason::DailyLossLimitBreached { .. })
        ));
    }

    /// `long_signal` usa o mesmo `StrategyId::new("test")` da posição
    /// abaixo — este teste prova que **o mesmo robô** ainda não pode abrir
    /// uma segunda posição no mesmo instrumento enquanto a primeira segue
    /// aberta (ADR-20 restringe por instrumento *e* robô, não removeu o
    /// limite por robô).
    #[test]
    fn rejects_second_entry_on_same_instrument_for_the_same_robot() {
        let engine = RiskEngine::new(base_config());
        let instrument = instrument();
        let signal = long_signal(instrument.id);
        let position = Position {
            id: Uuid::new_v4(),
            instrument_id: instrument.id,
            strategy_id: StrategyId::new("test").unwrap(),
            side: Side::Buy,
            quantity: dec!(0.01),
            entry_price: dec!(48000),
            exit_price: None,
            opened_at: Utc::now(),
            closed_at: None,
            status: PositionStatus::Open,
            realized_pnl_gross: None,
            realized_pnl_net: None,
            fees_paid: Decimal::ZERO,
            spread_paid: Decimal::ZERO,
            slippage_paid: Decimal::ZERO,
        };
        let positions = vec![position];
        let portfolio = PortfolioState {
            cash: Money::new(dec!(10000)),
            equity: Money::new(dec!(10000)),
            open_positions: &positions,
            realized_pnl_today: Money::zero(),
        };
        let price = Price::new(dec!(50000)).unwrap();

        let decision = engine.evaluate(&signal, &instrument, price, &portfolio);
        assert_eq!(
            decision,
            RiskDecision::Rejected(RejectionReason::PositionAlreadyOpen)
        );
    }

    fn position_for(
        instrument_id: domain::InstrumentId,
        strategy_id: StrategyId,
        entry_price: Decimal,
        quantity: Decimal,
    ) -> Position {
        Position {
            id: Uuid::new_v4(),
            instrument_id,
            strategy_id,
            side: Side::Buy,
            quantity,
            entry_price,
            exit_price: None,
            opened_at: Utc::now(),
            closed_at: None,
            status: PositionStatus::Open,
            realized_pnl_gross: None,
            realized_pnl_net: None,
            fees_paid: Decimal::ZERO,
            spread_paid: Decimal::ZERO,
            slippage_paid: Decimal::ZERO,
        }
    }

    /// Requisito 1: um robô diferente (`robot-b`) do dono da posição
    /// existente (`robot-a`) no mesmo instrumento é aprovado normalmente —
    /// `PositionAlreadyOpen` só se aplica a *este* robô, nunca a qualquer
    /// robô no instrumento.
    #[test]
    fn a_different_robot_is_approved_on_an_instrument_another_robot_already_holds() {
        let engine = RiskEngine::new(base_config());
        let instrument = instrument();
        let signal = Signal::new(
            StrategyId::new("robot-b").unwrap(),
            instrument.id,
            SignalDirection::Long,
            0.8,
            Utc::now(),
            None,
            None,
            serde_json::Value::Null,
        )
        .unwrap();
        let positions = vec![position_for(
            instrument.id,
            StrategyId::new("robot-a").unwrap(),
            dec!(48000),
            dec!(0.01),
        )];
        let portfolio = PortfolioState {
            cash: Money::new(dec!(10000)),
            equity: Money::new(dec!(10000)),
            open_positions: &positions,
            realized_pnl_today: Money::zero(),
        };
        let price = Price::new(dec!(50000)).unwrap();

        let decision = engine.evaluate(&signal, &instrument, price, &portfolio);
        assert!(
            decision.is_approved(),
            "robot-b must not be blocked by robot-a's unrelated position: {decision:?}"
        );
    }

    /// Requisito 4: a exposição usada para `ExposureLimitExceeded` é a
    /// soma das posições de TODOS os robôs, não só do robô que está
    /// sendo avaliado — duas posições de robôs diferentes já perto do
    /// limite fazem um terceiro robô ser rejeitado.
    #[test]
    fn exposure_limit_considers_positions_from_every_robot_combined() {
        let mut config = base_config();
        config.max_total_exposure = Money::new(dec!(1500));
        config.max_position_notional = Money::new(dec!(1000));
        let engine = RiskEngine::new(config);
        let instrument = instrument();
        let signal = Signal::new(
            StrategyId::new("robot-c").unwrap(),
            instrument.id,
            SignalDirection::Long,
            0.8,
            Utc::now(),
            None,
            None,
            serde_json::Value::Null,
        )
        .unwrap();
        // robot-a (500) + robot-b (500) = 1000 já comprometidos; a entrada
        // de robot-c (1000, o order_notional default) projetaria 2000,
        // acima do limite de 1500 — mesmo que robot-c, sozinho, nunca
        // tenha se aproximado do limite.
        let positions = vec![
            position_for(
                instrument.id,
                StrategyId::new("robot-a").unwrap(),
                dec!(50000),
                dec!(0.01),
            ),
            position_for(
                instrument.id,
                StrategyId::new("robot-b").unwrap(),
                dec!(50000),
                dec!(0.01),
            ),
        ];
        let portfolio = PortfolioState {
            cash: Money::new(dec!(100000)),
            equity: Money::new(dec!(100000)),
            open_positions: &positions,
            realized_pnl_today: Money::zero(),
        };
        let price = Price::new(dec!(50000)).unwrap();

        let decision = engine.evaluate(&signal, &instrument, price, &portfolio);
        assert!(matches!(
            decision,
            RiskDecision::Rejected(RejectionReason::ExposureLimitExceeded { .. })
        ));
    }

    /// Requisito 5: caixa já gasto por outros robôs é indisponível para um
    /// novo robô — o caixa é um único saldo compartilhado do portfólio,
    /// não um orçamento por robô.
    #[test]
    fn insufficient_balance_reflects_cash_already_spent_by_other_robots() {
        let engine = RiskEngine::new(base_config()); // order_notional = 1000
        let instrument = instrument();
        let signal = Signal::new(
            StrategyId::new("robot-b").unwrap(),
            instrument.id,
            SignalDirection::Long,
            0.8,
            Utc::now(),
            None,
            None,
            serde_json::Value::Null,
        )
        .unwrap();
        // robot-a já gastou a maior parte do caixa: só sobra 500, abaixo
        // do order_notional de 1000 que robot-b precisaria.
        let portfolio = PortfolioState {
            cash: Money::new(dec!(500)),
            equity: Money::new(dec!(10000)),
            open_positions: &[],
            realized_pnl_today: Money::zero(),
        };
        let price = Price::new(dec!(50000)).unwrap();

        let decision = engine.evaluate(&signal, &instrument, price, &portfolio);
        assert_eq!(
            decision,
            RiskDecision::Rejected(RejectionReason::InsufficientBalance {
                required: Money::new(dec!(1000)),
                available: Money::new(dec!(500)),
            })
        );
    }

    #[test]
    fn flat_signal_without_position_is_rejected() {
        let engine = RiskEngine::new(base_config());
        let instrument = instrument();
        let signal = Signal::new(
            StrategyId::new("test").unwrap(),
            instrument.id,
            SignalDirection::Flat,
            1.0,
            Utc::now(),
            None,
            None,
            serde_json::Value::Null,
        )
        .unwrap();
        let portfolio = empty_portfolio();
        let price = Price::new(dec!(50000)).unwrap();

        let decision = engine.evaluate(&signal, &instrument, price, &portfolio);
        assert_eq!(
            decision,
            RiskDecision::Rejected(RejectionReason::NothingToFlatten)
        );
    }

    #[test]
    fn short_signal_without_position_never_opens_a_naked_short() {
        let engine = RiskEngine::new(base_config());
        let instrument = instrument();
        let signal = short_signal(instrument.id);
        let portfolio = empty_portfolio();
        let price = Price::new(dec!(50000)).unwrap();

        let decision = engine.evaluate(&signal, &instrument, price, &portfolio);
        assert_eq!(
            decision,
            RiskDecision::Rejected(RejectionReason::ShortSellingNotSupported)
        );
    }

    #[test]
    fn short_signal_closes_an_existing_long_position() {
        let engine = RiskEngine::new(base_config());
        let instrument = instrument();
        let signal = short_signal(instrument.id);
        let position = Position {
            id: Uuid::new_v4(),
            instrument_id: instrument.id,
            strategy_id: StrategyId::new("test").unwrap(),
            side: Side::Buy,
            quantity: dec!(0.01),
            entry_price: dec!(48000),
            exit_price: None,
            opened_at: Utc::now(),
            closed_at: None,
            status: PositionStatus::Open,
            realized_pnl_gross: None,
            realized_pnl_net: None,
            fees_paid: Decimal::ZERO,
            spread_paid: Decimal::ZERO,
            slippage_paid: Decimal::ZERO,
        };
        let positions = vec![position];
        let portfolio = PortfolioState {
            cash: Money::new(dec!(10000)),
            equity: Money::new(dec!(10000)),
            open_positions: &positions,
            realized_pnl_today: Money::zero(),
        };
        let price = Price::new(dec!(50000)).unwrap();

        let decision = engine.evaluate(&signal, &instrument, price, &portfolio);
        assert!(decision.is_approved());
        if let RiskDecision::Approved(order) = decision {
            // Fechar um long é sempre um Sell, independentemente de o sinal
            // ser Short em vez de Flat.
            assert_eq!(order.side, Side::Sell);
            assert_eq!(order.quantity, dec!(0.01));
        }
    }

    #[test]
    fn stop_loss_triggers_on_long_position_below_threshold() {
        let engine = RiskEngine::new(base_config());
        let position = Position {
            id: Uuid::new_v4(),
            instrument_id: domain::InstrumentId::new(),
            strategy_id: StrategyId::new("test").unwrap(),
            side: Side::Buy,
            quantity: dec!(1),
            entry_price: dec!(100),
            exit_price: None,
            opened_at: Utc::now(),
            closed_at: None,
            status: PositionStatus::Open,
            realized_pnl_gross: None,
            realized_pnl_net: None,
            fees_paid: Decimal::ZERO,
            spread_paid: Decimal::ZERO,
            slippage_paid: Decimal::ZERO,
        };
        // Stop loss configurado em 5%; o preço em 94 é uma queda de 6%.
        assert_eq!(
            engine.check_exit(&position, dec!(94)),
            Some(ExitReason::StopLoss)
        );
        // O preço em 97 é uma queda de apenas 3%, dentro da tolerância.
        assert_eq!(engine.check_exit(&position, dec!(97)), None);
    }

    /// Auditoria semântica das estratégias baseadas em features: confirma,
    /// com evidência direta e não apenas por leitura de código, que
    /// `Signal::confidence` é uma força heurística declarada pela
    /// estratégia — não uma probabilidade de lucro que o motor de risco
    /// deveria (ou de fato) usar para dimensionar a ordem. Dois sinais
    /// idênticos exceto pela confiança (`0.05` vs. `0.99`) precisam
    /// produzir exatamente a mesma quantidade aprovada — se o
    /// dimensionamento algum dia passar a ler `confidence`, este teste
    /// falha e sinaliza a mudança de comportamento explicitamente.
    #[test]
    fn signal_confidence_does_not_affect_position_sizing() {
        let engine = RiskEngine::new(base_config());
        let instrument = instrument();
        let portfolio = empty_portfolio();
        let price = Price::new(dec!(50000)).unwrap();

        let low_confidence = Signal::new(
            StrategyId::new("test").unwrap(),
            instrument.id,
            SignalDirection::Long,
            0.05,
            Utc::now(),
            None,
            None,
            serde_json::Value::Null,
        )
        .unwrap();
        let high_confidence = Signal::new(
            StrategyId::new("test").unwrap(),
            instrument.id,
            SignalDirection::Long,
            0.99,
            Utc::now(),
            None,
            None,
            serde_json::Value::Null,
        )
        .unwrap();

        let low_decision = engine.evaluate(&low_confidence, &instrument, price, &portfolio);
        let high_decision = engine.evaluate(&high_confidence, &instrument, price, &portfolio);

        let (RiskDecision::Approved(low_order), RiskDecision::Approved(high_order)) =
            (low_decision, high_decision)
        else {
            panic!("both signals should be approved under identical portfolio/price conditions");
        };
        assert_eq!(
            low_order.quantity, high_order.quantity,
            "position size must depend only on RiskConfig::order_notional, never on signal.confidence"
        );
    }

    #[test]
    fn take_profit_triggers_on_long_position_above_threshold() {
        let engine = RiskEngine::new(base_config());
        let position = Position {
            id: Uuid::new_v4(),
            instrument_id: domain::InstrumentId::new(),
            strategy_id: StrategyId::new("test").unwrap(),
            side: Side::Buy,
            quantity: dec!(1),
            entry_price: dec!(100),
            exit_price: None,
            opened_at: Utc::now(),
            closed_at: None,
            status: PositionStatus::Open,
            realized_pnl_gross: None,
            realized_pnl_net: None,
            fees_paid: Decimal::ZERO,
            spread_paid: Decimal::ZERO,
            slippage_paid: Decimal::ZERO,
        };
        assert_eq!(
            engine.check_exit(&position, dec!(115)),
            Some(ExitReason::TakeProfit)
        );
    }
}
