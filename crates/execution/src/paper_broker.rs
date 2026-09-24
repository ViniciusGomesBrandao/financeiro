use async_trait::async_trait;
use domain::{
    notional, Fill, Instrument, Liquidity, Order, OrderStatus, OrderType, Price, Quantity, Side,
};
use portfolio::PortfolioManager;
use rust_decimal::Decimal;
use tracing::info;
use uuid::Uuid;

use crate::broker::Broker;
use crate::config::PaperBrokerConfig;
use crate::error::BrokerError;
use crate::report::{ExecutionReport, PositionEvent};

/// Um broker totalmente simulado: sem exchange, sem dinheiro real, nunca.
/// Ele existe para que o resto do sistema (risco, portfólio, persistência,
/// analytics) possa ser exercitado de ponta a ponta antes de qualquer
/// decisão sobre execução real.
///
/// Os fills são simulados, não perfeitos:
/// - todo fill tem seu preço afastado do preço de referência por dois
///   ajustes adversos distintos, `PaperBrokerConfig::spread_bps` (custo de
///   cruzar o bid/ask como taker) e `PaperBrokerConfig::slippage_bps`
///   (impacto de mercado adicional), registrados separadamente no `Fill`
///   resultante como `spread_cost`/`slippage_cost`;
/// - todo fill é cobrado em `PaperBrokerConfig::taker_fee` sobre o seu
///   notional;
/// - o preço de execução é quantizado ao `Instrument::tick_size`, e a
///   quantidade ao `Instrument::lot_size` (arredondada para baixo), via
///   `Instrument::round_price_to_tick`/`round_quantity_down_to_lot`.
///
/// `spread_cost`/`slippage_cost` são detalhamentos diagnósticos de um
/// ajuste que *já* está embutido no preço do fill — não são uma dedução
/// adicional. O P&L realizado (calculado em `portfolio` a partir dos preços
/// de entrada/saída) já desconta spread e slippage dessa forma; apenas
/// `fee` é um custo de caixa separado e explícito. É por isso que o custo
/// total simulado de negociação de um fill é `fee + spread_cost +
/// slippage_cost`, e não apenas `fee`, ainda que só `fee` seja subtraído
/// uma segunda vez (do caixa) em vez de estar implícito em uma diferença de
/// preço.
///
/// Apenas `OrderType::Market` é suportado. A simulação de ordens limit
/// (maker) exigiria modelar um livro de ofertas com ordens em repouso
/// contra dados em streaming, o que esta fase não tenta fazer — veja
/// `docs/architecture.md`.
pub struct PaperBroker {
    config: PaperBrokerConfig,
}

struct PriceBreakdown {
    execution_price: Decimal,
    /// Ajuste de preço por unidade atribuível ao spread (magnitude sempre
    /// positiva, independentemente do lado).
    spread_adjustment: Decimal,
    /// Ajuste de preço por unidade atribuível ao slippage (magnitude sempre
    /// positiva, independentemente do lado).
    slippage_adjustment: Decimal,
}

impl PaperBroker {
    pub fn new(config: PaperBrokerConfig) -> Self {
        Self { config }
    }

    /// Calcula o preço de execução simulado e o tamanho de cada ajuste
    /// adverso que o produziu. Ambos os ajustes são frações do mesmo preço
    /// de referência, aplicados de forma aditiva e sempre adversa: compras
    /// executam mais caro, vendas executam mais barato.
    fn price_breakdown(&self, side: Side, reference_price: Price) -> PriceBreakdown {
        let spread_adjustment =
            reference_price.value() * (self.config.spread_bps / Decimal::from(10_000));
        let slippage_adjustment =
            reference_price.value() * (self.config.slippage_bps / Decimal::from(10_000));
        let total_adjustment = spread_adjustment + slippage_adjustment;

        let execution_price = match side {
            Side::Buy => reference_price.value() + total_adjustment,
            Side::Sell => reference_price.value() - total_adjustment,
        };

        PriceBreakdown {
            execution_price,
            spread_adjustment,
            slippage_adjustment,
        }
    }
}

#[async_trait]
impl Broker for PaperBroker {
    async fn submit_order(
        &mut self,
        request: domain::OrderRequest,
        reference_price: Price,
        execution_time: chrono::DateTime<chrono::Utc>,
        instrument: &Instrument,
        portfolio: &mut PortfolioManager,
    ) -> Result<ExecutionReport, BrokerError> {
        if request.order_type != OrderType::Market {
            return Err(BrokerError::UnsupportedOrderType(request.order_type));
        }

        // Quantiza para o lot size do instrumento antes de executar
        // qualquer coisa. `risk::RiskEngine` já arredonda para baixo até o
        // lot size ao dimensionar uma ordem, mas o broker não confia nisso
        // como sua única linha de defesa — mesma postura da proteção
        // contra naked short abaixo.
        let rounded_quantity = instrument.round_quantity_down_to_lot(request.quantity);
        let quantity = Quantity::new(rounded_quantity).map_err(|_| BrokerError::InvalidQuantity)?;

        let breakdown = self.price_breakdown(request.side, reference_price);
        let execution_price_decimal = instrument.round_price_to_tick(breakdown.execution_price);
        let execution_price =
            Price::new(execution_price_decimal).map_err(|_| BrokerError::InvalidExecutionPrice)?;

        let spread_cost = breakdown.spread_adjustment * quantity.value();
        let slippage_cost = breakdown.slippage_adjustment * quantity.value();
        let fee = self.config.taker_fee * notional(execution_price, quantity).value();

        // Deliberadamente `execution_time`, não `request.requested_at`: o
        // fill (e a posição que ele abre/fecha) aconteceu quando o broker
        // agiu, não quando a decisão foi tomada — ver o doc de `Broker`.
        // `Order::created_at` (abaixo, via `Order::new`) continua vindo de
        // `request.requested_at`, preservando os dois timestamps
        // separados no resultado.
        let now = execution_time;
        let existing = portfolio
            .open_position_for(request.instrument_id, &request.strategy_id)
            .cloned();

        let position_event = match existing {
            None if request.side == Side::Sell => {
                // Conta spot: abrir uma posição com uma ordem Sell
                // significa vender um ativo que o portfólio não possui (um
                // naked short). Não existe mecanismo de empréstimo/margem
                // aqui, então isso é recusado incondicionalmente em vez de
                // ser executado silenciosamente — embora `risk::RiskEngine`
                // nunca devesse produzir tal requisição, o broker não
                // confia nisso como sua única linha de defesa.
                return Err(BrokerError::NakedShortNotSupported);
            }
            None => {
                let position = portfolio.open_position(
                    request.instrument_id,
                    request.strategy_id.clone(),
                    request.side,
                    quantity.value(),
                    execution_price.value(),
                    fee,
                    spread_cost,
                    slippage_cost,
                    now,
                );
                PositionEvent::Opened(position)
            }
            Some(existing) if existing.side == request.side.opposite() => {
                let position = portfolio.close_position(
                    request.instrument_id,
                    &request.strategy_id,
                    execution_price.value(),
                    fee,
                    spread_cost,
                    slippage_cost,
                    now,
                )?;
                PositionEvent::Closed(position)
            }
            Some(_) => return Err(BrokerError::UnexpectedSameSideOrder),
        };

        let mut order = Order::new(&request);
        order.status = OrderStatus::Filled;
        order.filled_quantity = quantity.value();
        order.average_fill_price = Some(execution_price.value());
        order.updated_at = now;

        let fill = Fill {
            id: Uuid::new_v4(),
            order_id: order.id,
            price: execution_price.value(),
            quantity: quantity.value(),
            fee,
            spread_cost,
            slippage_cost,
            liquidity: Liquidity::Taker,
            executed_at: now,
        };

        info!(
            instrument_id = ?request.instrument_id,
            side = ?request.side,
            quantity = %quantity,
            execution_price = %execution_price,
            fee = %fee,
            spread_cost = %spread_cost,
            slippage_cost = %slippage_cost,
            "paper order executed"
        );

        Ok(ExecutionReport {
            order,
            fill,
            position_event,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use domain::{
        AssetClass, Exchange, InstrumentId, MarketType, OrderRequest, StrategyId, Symbol,
    };
    use rust_decimal_macros::dec;

    fn config() -> PaperBrokerConfig {
        PaperBrokerConfig {
            maker_fee: dec!(0.0005),
            taker_fee: dec!(0.001),
            // 2 bps de spread + 3 bps de slippage = 5 bps no total,
            // correspondendo ao ajuste combinado que esta suíte usava
            // antes de os dois serem modelados separadamente.
            spread_bps: dec!(2),
            slippage_bps: dec!(3),
        }
    }

    /// tick_size = 0.01, lot_size = 0.0001 — granular o suficiente para que
    /// nenhum dos preços/quantidades calculados à mão nesta suíte caia em
    /// uma fronteira de arredondamento, de modo que os valores esperados
    /// existentes permaneçam exatos.
    fn instrument() -> Instrument {
        let base = domain::Asset::new("BTC").unwrap();
        let quote = domain::Asset::new("USDT").unwrap();
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

    fn order_request(instrument_id: InstrumentId, side: Side, quantity: Decimal) -> OrderRequest {
        order_request_for(instrument_id, "test", side, quantity)
    }

    fn order_request_for(
        instrument_id: InstrumentId,
        strategy_id: &str,
        side: Side,
        quantity: Decimal,
    ) -> OrderRequest {
        OrderRequest {
            instrument_id,
            strategy_id: StrategyId::new(strategy_id).unwrap(),
            signal_id: None,
            side,
            order_type: OrderType::Market,
            quantity,
            limit_price: None,
            requested_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn buy_fills_above_reference_price_due_to_slippage() {
        let mut broker = PaperBroker::new(config());
        let mut portfolio = PortfolioManager::new(domain::Money::new(dec!(100000)));
        let instrument = instrument();
        let reference = Price::new(dec!(50000)).unwrap();

        let report = broker
            .submit_order(
                order_request(instrument.id, Side::Buy, dec!(1)),
                reference,
                Utc::now(),
                &instrument,
                &mut portfolio,
            )
            .await
            .unwrap();

        // 2 bps de spread + 3 bps de slippage sobre 50000 = 10 + 15 = 25,
        // então o preço de execução = 50025.
        assert_eq!(report.fill.price, dec!(50025));
        assert!(matches!(report.position_event, PositionEvent::Opened(_)));
        assert_eq!(portfolio.open_positions().len(), 1);
    }

    #[tokio::test]
    async fn sell_fills_below_reference_price_due_to_slippage() {
        let mut broker = PaperBroker::new(config());
        let mut portfolio = PortfolioManager::new(domain::Money::new(dec!(100000)));
        let instrument = instrument();
        let reference = Price::new(dec!(50000)).unwrap();

        // Spot: uma Sell só fecha uma posição long existente, então é
        // preciso abrir uma primeiro.
        broker
            .submit_order(
                order_request(instrument.id, Side::Buy, dec!(1)),
                reference,
                Utc::now(),
                &instrument,
                &mut portfolio,
            )
            .await
            .unwrap();

        let report = broker
            .submit_order(
                order_request(instrument.id, Side::Sell, dec!(1)),
                reference,
                Utc::now(),
                &instrument,
                &mut portfolio,
            )
            .await
            .unwrap();

        assert_eq!(report.fill.price, dec!(49975));
    }

    #[tokio::test]
    async fn records_spread_and_slippage_cost_separately() {
        let mut broker = PaperBroker::new(config());
        let mut portfolio = PortfolioManager::new(domain::Money::new(dec!(100000)));
        let instrument = instrument();
        let reference = Price::new(dec!(50000)).unwrap();

        let report = broker
            .submit_order(
                order_request(instrument.id, Side::Buy, dec!(1)),
                reference,
                Utc::now(),
                &instrument,
                &mut portfolio,
            )
            .await
            .unwrap();

        // spread: 50000 * 0.0002 * 1 = 10; slippage: 50000 * 0.0003 * 1 = 15.
        assert_eq!(report.fill.spread_cost, dec!(10));
        assert_eq!(report.fill.slippage_cost, dec!(15));
        assert_eq!(
            report.fill.spread_cost + report.fill.slippage_cost,
            dec!(25)
        );

        let position = portfolio.open_positions().first().unwrap();
        assert_eq!(position.spread_paid, dec!(10));
        assert_eq!(position.slippage_paid, dec!(15));
    }

    #[tokio::test]
    async fn execution_price_is_rounded_to_tick_size() {
        let mut broker = PaperBroker::new(config());
        let mut portfolio = PortfolioManager::new(domain::Money::new(dec!(100000)));
        let mut instrument = instrument();
        instrument.tick_size = dec!(1); // apenas dólares inteiros

        // 2 bps + 3 bps sobre 50003 = 25.0015, preço de execução =
        // 50028.0015, que deve arredondar para o dólar inteiro mais
        // próximo: 50028.
        let reference = Price::new(dec!(50003)).unwrap();

        let report = broker
            .submit_order(
                order_request(instrument.id, Side::Buy, dec!(1)),
                reference,
                Utc::now(),
                &instrument,
                &mut portfolio,
            )
            .await
            .unwrap();

        assert_eq!(report.fill.price, dec!(50028));
    }

    #[tokio::test]
    async fn quantity_is_rounded_down_to_lot_size() {
        let mut broker = PaperBroker::new(config());
        let mut portfolio = PortfolioManager::new(domain::Money::new(dec!(100000)));
        let mut instrument = instrument();
        instrument.lot_size = dec!(0.001);
        let reference = Price::new(dec!(50000)).unwrap();

        let report = broker
            .submit_order(
                // 0.0114 não é múltiplo de 0.001; deve ser truncado para 0.011.
                order_request(instrument.id, Side::Buy, dec!(0.0114)),
                reference,
                Utc::now(),
                &instrument,
                &mut portfolio,
            )
            .await
            .unwrap();

        assert_eq!(report.fill.quantity, dec!(0.011));
    }

    #[tokio::test]
    async fn rejects_quantity_that_rounds_down_to_zero() {
        let mut broker = PaperBroker::new(config());
        let mut portfolio = PortfolioManager::new(domain::Money::new(dec!(100000)));
        let mut instrument = instrument();
        instrument.lot_size = dec!(1); // apenas unidades inteiras
        let reference = Price::new(dec!(50000)).unwrap();

        let result = broker
            .submit_order(
                order_request(instrument.id, Side::Buy, dec!(0.5)),
                reference,
                Utc::now(),
                &instrument,
                &mut portfolio,
            )
            .await;

        assert!(matches!(result, Err(BrokerError::InvalidQuantity)));
    }

    #[tokio::test]
    async fn rejects_naked_short_when_no_position_is_held() {
        let mut broker = PaperBroker::new(config());
        let mut portfolio = PortfolioManager::new(domain::Money::new(dec!(100000)));
        let instrument = instrument();
        let reference = Price::new(dec!(50000)).unwrap();

        let result = broker
            .submit_order(
                order_request(instrument.id, Side::Sell, dec!(1)),
                reference,
                Utc::now(),
                &instrument,
                &mut portfolio,
            )
            .await;

        assert!(matches!(result, Err(BrokerError::NakedShortNotSupported)));
        assert!(portfolio.open_positions().is_empty());
    }

    #[tokio::test]
    async fn charges_taker_fee_on_notional() {
        let mut broker = PaperBroker::new(config());
        let mut portfolio = PortfolioManager::new(domain::Money::new(dec!(100000)));
        let instrument = instrument();
        let reference = Price::new(dec!(1000)).unwrap();

        let report = broker
            .submit_order(
                order_request(instrument.id, Side::Buy, dec!(1)),
                reference,
                Utc::now(),
                &instrument,
                &mut portfolio,
            )
            .await
            .unwrap();

        // preço de execução = 1000 * 1.0005 = 1000.5, fee = 0.001 * 1000.5 = 1.0005
        assert_eq!(report.fill.fee, dec!(1.0005));
    }

    #[tokio::test]
    async fn opposite_side_order_closes_existing_position() {
        let mut broker = PaperBroker::new(config());
        let mut portfolio = PortfolioManager::new(domain::Money::new(dec!(100000)));
        let instrument = instrument();
        let reference = Price::new(dec!(50000)).unwrap();

        broker
            .submit_order(
                order_request(instrument.id, Side::Buy, dec!(1)),
                reference,
                Utc::now(),
                &instrument,
                &mut portfolio,
            )
            .await
            .unwrap();

        let report = broker
            .submit_order(
                order_request(instrument.id, Side::Sell, dec!(1)),
                Price::new(dec!(51000)).unwrap(),
                Utc::now(),
                &instrument,
                &mut portfolio,
            )
            .await
            .unwrap();

        assert!(matches!(report.position_event, PositionEvent::Closed(_)));
        assert!(portfolio.open_positions().is_empty());
        assert_eq!(portfolio.closed_positions().len(), 1);
    }

    #[tokio::test]
    async fn rejects_limit_orders() {
        let mut broker = PaperBroker::new(config());
        let mut portfolio = PortfolioManager::new(domain::Money::new(dec!(100000)));
        let instrument = instrument();
        let mut request = order_request(instrument.id, Side::Buy, dec!(1));
        request.order_type = OrderType::Limit;
        request.limit_price = Some(dec!(50000));

        let result = broker
            .submit_order(
                request,
                Price::new(dec!(50000)).unwrap(),
                Utc::now(),
                &instrument,
                &mut portfolio,
            )
            .await;
        assert!(matches!(result, Err(BrokerError::UnsupportedOrderType(_))));
    }

    #[tokio::test]
    async fn rejects_same_side_order_on_open_position() {
        let mut broker = PaperBroker::new(config());
        let mut portfolio = PortfolioManager::new(domain::Money::new(dec!(100000)));
        let instrument = instrument();
        let reference = Price::new(dec!(50000)).unwrap();

        broker
            .submit_order(
                order_request(instrument.id, Side::Buy, dec!(1)),
                reference,
                Utc::now(),
                &instrument,
                &mut portfolio,
            )
            .await
            .unwrap();

        let result = broker
            .submit_order(
                order_request(instrument.id, Side::Buy, dec!(1)),
                reference,
                Utc::now(),
                &instrument,
                &mut portfolio,
            )
            .await;
        assert!(matches!(result, Err(BrokerError::UnexpectedSameSideOrder)));
    }

    /// `request.requested_at` (quando a decisão foi tomada) e
    /// `execution_time` (quando o broker de fato agiu) são dois campos
    /// deliberadamente diferentes — ver o doc de `Broker::submit_order`.
    /// Passa dois valores bem distintos e confirma que cada um chega ao
    /// campo certo do resultado: `Order::created_at` reflete a decisão,
    /// `Fill::executed_at`/`Order::updated_at`/o timestamp da posição
    /// refletem a execução — nunca o mesmo valor reaproveitado duas vezes.
    #[tokio::test]
    async fn order_created_at_and_fill_executed_at_come_from_different_timestamps() {
        let mut broker = PaperBroker::new(config());
        let mut portfolio = PortfolioManager::new(domain::Money::new(dec!(100000)));
        let instrument = instrument();
        let reference = Price::new(dec!(50000)).unwrap();

        let signal_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let execution_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 1, 0).unwrap();
        let mut request = order_request(instrument.id, Side::Buy, dec!(1));
        request.requested_at = signal_time;

        let report = broker
            .submit_order(
                request,
                reference,
                execution_time,
                &instrument,
                &mut portfolio,
            )
            .await
            .unwrap();

        assert_eq!(report.order.created_at, signal_time);
        assert_eq!(report.order.updated_at, execution_time);
        assert_eq!(report.fill.executed_at, execution_time);
        assert_ne!(
            report.order.created_at, report.order.updated_at,
            "signal and execution timestamps must never collapse into the same value when they \
             were given as different values"
        );

        let position = portfolio.open_positions().first().unwrap();
        assert_eq!(
            position.opened_at, execution_time,
            "a position must be timestamped when it was actually opened (executed), not when \
             the signal that led to it was decided"
        );
    }

    /// Requisito 1: dois robôs (`strategy_id` diferentes) abrem posições no
    /// mesmo instrumento através do mesmo broker, sem que o segundo seja
    /// tratado como "mesmo lado repetido" nem "fechamento" do primeiro.
    #[tokio::test]
    async fn two_robots_open_independent_positions_on_the_same_instrument() {
        let mut broker = PaperBroker::new(config());
        let mut portfolio = PortfolioManager::new(domain::Money::new(dec!(100000)));
        let instrument = instrument();
        let reference = Price::new(dec!(50000)).unwrap();

        broker
            .submit_order(
                order_request_for(instrument.id, "robot-a", Side::Buy, dec!(1)),
                reference,
                Utc::now(),
                &instrument,
                &mut portfolio,
            )
            .await
            .unwrap();
        let report_b = broker
            .submit_order(
                order_request_for(instrument.id, "robot-b", Side::Buy, dec!(1)),
                reference,
                Utc::now(),
                &instrument,
                &mut portfolio,
            )
            .await
            .unwrap();

        assert!(matches!(report_b.position_event, PositionEvent::Opened(_)));
        assert_eq!(portfolio.open_positions().len(), 2);
    }

    /// Requisitos 2/3: fechar a posição de robot-a (ordem de lado oposto,
    /// mesmo instrumento) não fecha nem altera a posição de robot-b.
    #[tokio::test]
    async fn closing_one_robots_position_leaves_the_others_open() {
        let mut broker = PaperBroker::new(config());
        let mut portfolio = PortfolioManager::new(domain::Money::new(dec!(100000)));
        let instrument = instrument();
        let reference = Price::new(dec!(50000)).unwrap();

        broker
            .submit_order(
                order_request_for(instrument.id, "robot-a", Side::Buy, dec!(1)),
                reference,
                Utc::now(),
                &instrument,
                &mut portfolio,
            )
            .await
            .unwrap();
        broker
            .submit_order(
                order_request_for(instrument.id, "robot-b", Side::Buy, dec!(1)),
                reference,
                Utc::now(),
                &instrument,
                &mut portfolio,
            )
            .await
            .unwrap();

        let report = broker
            .submit_order(
                order_request_for(instrument.id, "robot-a", Side::Sell, dec!(1)),
                Price::new(dec!(51000)).unwrap(),
                Utc::now(),
                &instrument,
                &mut portfolio,
            )
            .await
            .unwrap();

        assert!(matches!(report.position_event, PositionEvent::Closed(_)));
        assert_eq!(portfolio.open_positions().len(), 1);
        assert_eq!(
            portfolio.open_positions()[0].strategy_id,
            StrategyId::new("robot-b").unwrap(),
            "robot-b's position must remain open and untouched"
        );
    }
}
