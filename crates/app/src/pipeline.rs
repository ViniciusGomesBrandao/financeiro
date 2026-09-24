use std::collections::HashMap;

use anyhow::Result;
use chrono::Utc;
use domain::{Candle, Instrument, InstrumentId, MarketEvent, Order, Position, Price, StrategyId};
use execution::{Broker, PositionEvent};
use persistence::risk_decisions::RiskDecisionRecord;
use portfolio::PortfolioManager;
use risk::{ExitReason, RejectionReason, RiskDecision, RiskEngine};
use sqlx::PgPool;
use strategies::StrategyRegistry;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::robot_market::RobotMarketView;
use crate::robot_runtime::RobotContext;
use crate::strategy_switch::{self, ActiveStrategyTracker};

/// Aquecimento por robô: cada instância só recebe candles do **seu** timeframe.
/// Também aquece o `FeatureEngine` do Judge contextual.
pub async fn warm_up_robots(
    registry: &mut StrategyRegistry,
    provider: &market_data::BinanceMarketData,
    instruments: &HashMap<InstrumentId, Instrument>,
    robots: &[RobotContext],
    market_views: &mut HashMap<String, RobotMarketView>,
    limit: u32,
) -> Result<()> {
    use market_data::MarketDataProvider;

    for robot in robots {
        let Some(instrument) = instruments.get(&robot.instrument_id) else {
            continue;
        };
        let candles = provider
            .fetch_recent_candles(instrument, robot.timeframe, limit)
            .await?;
        info!(
            robot_id = %robot.id,
            symbol = %instrument.symbol,
            timeframe = %robot.timeframe,
            count = candles.len(),
            "warming up robot strategy state"
        );
        for candle in candles {
            if let Some(view) = market_views.get_mut(&robot.id) {
                let _ = view.on_candle(&candle);
            }
            let _ = registry.dispatch_ids(
                instrument,
                &MarketEvent::Candle(candle),
                &strategies::NoPositions,
                &robot.strategy_ids,
            );
        }
    }
    Ok(())
}

/// Após o warm-up, persiste a primeira avaliação contextual do Judge para
/// cada robô — assim o dashboard não fica em idle até o próximo candle live.
pub async fn seed_judge_after_warmup(
    instruments: &HashMap<InstrumentId, Instrument>,
    robots: &[RobotContext],
    tracker: &mut ActiveStrategyTracker,
    portfolios: &HashMap<String, PortfolioManager>,
    market_views: &HashMap<String, RobotMarketView>,
    pool: &PgPool,
) -> Result<()> {
    for robot in robots {
        let Some(instrument) = instruments.get(&robot.instrument_id) else {
            continue;
        };
        let Some(portfolio) = portfolios.get(&robot.id) else {
            warn!(robot_id = %robot.id, "portfolio missing; skip judge seed");
            continue;
        };
        let Some(view) = market_views.get(&robot.id) else {
            continue;
        };
        let Some((snap, prev_bw)) = view.last_for_judge() else {
            info!(
                robot_id = %robot.id,
                "no market features after warmup; skip judge seed"
            );
            continue;
        };

        let open_positions: Vec<Position> = portfolio
            .open_positions_for_instrument(robot.instrument_id)
            .into_iter()
            .filter(|p| robot.owns_strategy(&p.strategy_id))
            .cloned()
            .collect();

        let as_of = Utc::now();
        let update = tracker.update(
            &robot.id,
            robot.instrument_id,
            &robot.strategy_ids,
            portfolio.closed_positions(),
            &open_positions,
            as_of,
            Some(snap),
            prev_bw,
        );

        info!(
            robot_id = %robot.id,
            symbol = %instrument.symbol,
            regime = %update.regime.regime.as_str(),
            selected = ?update.selected.as_ref().map(|s| s.as_str()),
            "strategy judge: seeded after warmup"
        );
        persist_judge_telemetry(pool, &robot.id, &update).await?;
    }
    Ok(())
}

/// Pipeline live multi-robô: cada candle só alimenta robôs cujo
/// `(instrument, timeframe)` coincide; cada robô tem portfolio e Judge próprios.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    mut rx: mpsc::UnboundedReceiver<MarketEvent>,
    instruments: &HashMap<InstrumentId, Instrument>,
    robots: &[RobotContext],
    registry: &mut StrategyRegistry,
    tracker: &mut ActiveStrategyTracker,
    risk_engine: &RiskEngine,
    broker: &mut dyn Broker,
    portfolios: &mut HashMap<String, PortfolioManager>,
    market_views: &mut HashMap<String, RobotMarketView>,
    pool: &PgPool,
    mut shutdown: tokio::sync::oneshot::Receiver<()>,
) -> Result<()> {
    let mut mark_prices: HashMap<InstrumentId, rust_decimal::Decimal> = HashMap::new();

    loop {
        let event = tokio::select! {
            biased;
            _ = &mut shutdown => {
                info!("shutdown signal received, stopping pipeline");
                break;
            }
            event = rx.recv() => match event {
                Some(event) => event,
                None => {
                    warn!("market data stream ended");
                    break;
                }
            },
        };

        let MarketEvent::Candle(candle) = &event else {
            continue;
        };
        if !candle.is_closed {
            continue;
        }
        let Some(instrument) = instruments.get(&candle.instrument_id) else {
            continue;
        };

        mark_prices.insert(candle.instrument_id, candle.close);
        persistence::latest_prices::upsert(
            pool,
            candle.instrument_id,
            candle.close,
            candle.close_time,
        )
        .await?;
        info!(
            symbol = %instrument.symbol,
            timeframe = %candle.timeframe,
            close = %candle.close,
            "market event: candle closed"
        );

        let matching: Vec<&RobotContext> = robots
            .iter()
            .filter(|r| r.instrument_id == candle.instrument_id && r.timeframe == candle.timeframe)
            .collect();

        if matching.is_empty() {
            debug!(
                symbol = %instrument.symbol,
                timeframe = %candle.timeframe,
                "no robot registered for this instrument/timeframe candle"
            );
            continue;
        }

        let disabled: std::collections::HashSet<StrategyId> =
            persistence::strategy_configs::list_disabled_ids(pool)
                .await?
                .into_iter()
                .collect();

        for robot in matching {
            let Some(portfolio) = portfolios.get_mut(&robot.id) else {
                warn!(robot_id = %robot.id, "portfolio missing for robot; skipping candle");
                continue;
            };

            let market_view = market_views.get_mut(&robot.id);
            process_robot_candle(
                candle,
                instrument,
                robot,
                registry,
                tracker,
                risk_engine,
                broker,
                portfolio,
                market_view,
                pool,
                &disabled,
                &mark_prices,
            )
            .await?;
        }
    }

    for (robot_id, portfolio) in portfolios.iter() {
        let snapshot = portfolio.snapshot(&mark_prices, Utc::now());
        persistence::robot_portfolio_snapshots::insert(pool, robot_id, &snapshot).await?;
        info!(
            robot_id = %robot_id,
            equity = %snapshot.equity,
            realized_pnl = %snapshot.realized_pnl,
            "final robot portfolio snapshot saved"
        );
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn process_robot_candle(
    candle: &Candle,
    instrument: &Instrument,
    robot: &RobotContext,
    registry: &mut StrategyRegistry,
    tracker: &mut ActiveStrategyTracker,
    risk_engine: &RiskEngine,
    broker: &mut dyn Broker,
    portfolio: &mut PortfolioManager,
    market_view: Option<&mut RobotMarketView>,
    pool: &PgPool,
    disabled: &std::collections::HashSet<StrategyId>,
    mark_prices: &HashMap<InstrumentId, rust_decimal::Decimal>,
) -> Result<()> {
    let market_feed = market_view.and_then(|view| view.on_candle(candle));
    let (market_snap, prev_bw) = match &market_feed {
        Some((snap, prev)) => (Some(snap), *prev),
        None => (None, None),
    };

    let active_strategy_update = handle_strategy_switch(
        candle,
        instrument,
        robot,
        tracker,
        risk_engine,
        broker,
        portfolio,
        pool,
        market_snap,
        prev_bw,
    )
    .await?;

    handle_risk_driven_exit(
        candle,
        instrument,
        &robot.strategy_ids,
        risk_engine,
        broker,
        portfolio,
        pool,
    )
    .await?;

    let signals = registry.dispatch_ids(
        instrument,
        &MarketEvent::Candle(candle.clone()),
        &*portfolio,
        &robot.strategy_ids,
    );

    for signal in signals {
        if disabled.contains(&signal.strategy_id)
            && matches!(signal.direction, domain::SignalDirection::Long)
        {
            debug!(
                strategy_id = %signal.strategy_id,
                robot_id = %robot.id,
                "strategy disabled via dashboard: Long blocked"
            );
            continue;
        }
        if !strategy_switch::signal_allowed(&active_strategy_update, &signal) {
            debug!(
                strategy_id = %signal.strategy_id,
                robot_id = %robot.id,
                "strategy judge: signal blocked for this robot"
            );
            continue;
        }
        persistence::signals::insert(pool, &signal).await?;

        let price = Price::new(candle.close)?;
        let risk_snapshot = portfolio.risk_snapshot(candle.close_time);
        let decision = risk_engine.evaluate(&signal, instrument, price, &risk_snapshot);

        match decision {
            RiskDecision::Approved(order_request) => {
                let report = broker
                    .submit_order(
                        order_request,
                        price,
                        candle.close_time,
                        instrument,
                        portfolio,
                    )
                    .await?;
                persist_execution(pool, &report.order, &report.fill, &report.position_event)
                    .await?;
                persist_risk_decision(
                    pool,
                    Some(signal.id.0),
                    instrument.id,
                    &signal.strategy_id,
                    "signal",
                    true,
                    None,
                    Some(report.order.id),
                    candle.close_time,
                )
                .await?;
                log_position_event(instrument, &report.position_event);
            }
            RiskDecision::Rejected(reason) => {
                // Rejeição não altera portfolio nem estado interno de estratégia
                // (estratégias leem posição só via PositionQuery do portfolio).
                persist_risk_decision(
                    pool,
                    Some(signal.id.0),
                    instrument.id,
                    &signal.strategy_id,
                    "signal",
                    false,
                    Some(translate_rejection_reason(&reason)),
                    None,
                    candle.close_time,
                )
                .await?;
            }
        }
    }

    let snapshot = portfolio.snapshot(mark_prices, candle.close_time);
    persistence::robot_portfolio_snapshots::insert(pool, &robot.id, &snapshot).await?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_strategy_switch(
    candle: &Candle,
    instrument: &Instrument,
    robot: &RobotContext,
    tracker: &mut ActiveStrategyTracker,
    risk_engine: &RiskEngine,
    broker: &mut dyn Broker,
    portfolio: &mut PortfolioManager,
    pool: &PgPool,
    market: Option<&features::FeatureSnapshot>,
    prev_bandwidth: Option<f64>,
) -> Result<strategy_switch::ActiveStrategyUpdate> {
    let open_positions: Vec<Position> = portfolio
        .open_positions_for_instrument(candle.instrument_id)
        .into_iter()
        .filter(|p| robot.owns_strategy(&p.strategy_id))
        .cloned()
        .collect();

    let update = tracker.update(
        &robot.id,
        candle.instrument_id,
        &robot.strategy_ids,
        portfolio.closed_positions(),
        &open_positions,
        candle.close_time,
        market,
        prev_bandwidth,
    );

    if let Some(switch) = &update.switch {
        warn!(
            robot_id = %robot.id,
            instrument_id = ?switch.instrument_id,
            previous = ?switch.previous,
            new = ?switch.new,
            regime = %update.regime.regime.as_str(),
            reason = ?switch.reason,
            "strategy judge: switching active strategy for this robot"
        );
    } else {
        info!(
            robot_id = %robot.id,
            regime = %update.regime.regime.as_str(),
            strength = update.regime.strength,
            selected = ?update.selected.as_ref().map(|s| s.as_str()),
            "strategy judge: contextual evaluation"
        );
    }

    for position in &update.positions_to_close {
        let exit_order = risk_engine.build_exit_order(position, candle.close_time);
        let price = Price::new(candle.close)?;
        let report = broker
            .submit_order(exit_order, price, candle.close_time, instrument, portfolio)
            .await?;
        persist_execution(pool, &report.order, &report.fill, &report.position_event).await?;
        persist_risk_decision(
            pool,
            None,
            instrument.id,
            &position.strategy_id,
            "strategy_switch",
            true,
            Some("estratégia destituída pelo Strategy Judge".to_string()),
            Some(report.order.id),
            candle.close_time,
        )
        .await?;
        log_position_event(instrument, &report.position_event);
    }

    persist_judge_telemetry(pool, &robot.id, &update).await?;
    Ok(update)
}

async fn persist_judge_telemetry(
    pool: &PgPool,
    robot_id: &str,
    update: &strategy_switch::ActiveStrategyUpdate,
) -> Result<()> {
    use crate::judge_codec;

    let selected = update.selected.as_ref().map(|s| s.as_str());
    let evaluated_at = update
        .decisions
        .first()
        .map(|d| d.timestamp)
        .unwrap_or_else(Utc::now);

    persistence::active_strategy_state::upsert_active_state(
        pool,
        robot_id,
        update.instrument_id,
        selected,
        evaluated_at,
    )
    .await?;

    let decisions_json = judge_codec::encode_evaluation_payload(update);
    persistence::judge_telemetry::insert_evaluation(
        pool,
        update.instrument_id,
        Some(robot_id),
        evaluated_at,
        selected,
        &decisions_json,
    )
    .await?;

    if let Some(switch) = &update.switch {
        let reason_json = judge_codec::encode_switch_reason(update);
        persistence::judge_telemetry::insert_switch(
            pool,
            switch.instrument_id,
            Some(robot_id),
            switch.previous.as_ref().map(|s| s.as_str()),
            switch.new.as_ref().map(|s| s.as_str()),
            &reason_json,
            switch.timestamp,
        )
        .await?;
    }

    Ok(())
}

async fn handle_risk_driven_exit(
    candle: &Candle,
    instrument: &Instrument,
    strategy_ids: &[StrategyId],
    risk_engine: &RiskEngine,
    broker: &mut dyn Broker,
    portfolio: &mut PortfolioManager,
    pool: &PgPool,
) -> Result<()> {
    let positions: Vec<domain::Position> = portfolio
        .open_positions_for_instrument(candle.instrument_id)
        .into_iter()
        .filter(|p| strategy_ids.iter().any(|s| s == &p.strategy_id))
        .cloned()
        .collect();

    for position in positions {
        let Some(reason) = risk_engine.check_exit(&position, candle.close) else {
            continue;
        };

        info!(
            symbol = %instrument.symbol,
            strategy_id = %position.strategy_id,
            reason = ?reason,
            "risk-driven exit triggered"
        );
        let exit_order = risk_engine.build_exit_order(&position, candle.close_time);
        let price = Price::new(candle.close)?;
        let report = broker
            .submit_order(exit_order, price, candle.close_time, instrument, portfolio)
            .await?;

        persist_execution(pool, &report.order, &report.fill, &report.position_event).await?;

        let (trigger, trigger_label_pt) = match reason {
            ExitReason::StopLoss => ("stop_loss", "stop loss acionado"),
            ExitReason::TakeProfit => ("take_profit", "take profit acionado"),
        };
        persist_risk_decision(
            pool,
            None,
            instrument.id,
            &position.strategy_id,
            trigger,
            true,
            Some(trigger_label_pt.to_string()),
            Some(report.order.id),
            candle.close_time,
        )
        .await?;
        log_position_event(instrument, &report.position_event);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn persist_risk_decision(
    pool: &PgPool,
    signal_id: Option<Uuid>,
    instrument_id: InstrumentId,
    strategy_id: &domain::StrategyId,
    trigger: &str,
    approved: bool,
    reason: Option<String>,
    order_id: Option<Uuid>,
    created_at: chrono::DateTime<Utc>,
) -> Result<()> {
    persistence::risk_decisions::insert(
        pool,
        &RiskDecisionRecord {
            id: Uuid::new_v4(),
            signal_id: signal_id.map(domain::SignalId),
            instrument_id,
            strategy_id: strategy_id.clone(),
            trigger: trigger.to_string(),
            approved,
            reason,
            order_id,
            created_at,
        },
    )
    .await?;
    Ok(())
}

async fn persist_execution(
    pool: &PgPool,
    order: &Order,
    fill: &domain::Fill,
    position_event: &PositionEvent,
) -> Result<()> {
    persistence::orders::insert(pool, order).await?;
    persistence::fills::insert(pool, fill).await?;

    match position_event {
        PositionEvent::Opened(position) => {
            persistence::positions::upsert(pool, position).await?;
        }
        PositionEvent::Closed(position) => {
            persistence::positions::upsert(pool, position).await?;
            if let Some(trade) =
                persistence::trades::ClosedTradeRecord::from_closed_position(position, order.id)
            {
                persistence::trades::insert(pool, &trade).await?;
            }
        }
    }
    Ok(())
}

fn translate_rejection_reason(reason: &RejectionReason) -> String {
    match reason {
        RejectionReason::NothingToFlatten => "nenhuma posição aberta para zerar".to_string(),
        RejectionReason::ShortSellingNotSupported => {
            "venda a descoberto não é suportada (mercado à vista); não há posição para vender"
                .to_string()
        }
        RejectionReason::PositionAlreadyOpen => {
            "já existe uma posição aberta para este ativo".to_string()
        }
        RejectionReason::MaxOpenPositionsReached { current, max } => {
            format!("limite de posições abertas atingido ({current}/{max})")
        }
        RejectionReason::PositionSizeExceeded { requested, max } => {
            format!("tamanho da posição {requested} excede o máximo permitido {max}")
        }
        RejectionReason::ExposureLimitExceeded { projected, max } => {
            format!("exposição projetada {projected} excede o máximo permitido {max}")
        }
        RejectionReason::InsufficientBalance {
            required,
            available,
        } => format!("saldo insuficiente: necessário {required}, disponível {available}"),
        RejectionReason::DailyLossLimitBreached {
            realized_today,
            limit,
        } => format!("limite de perda diária atingido: realizado {realized_today}, limite {limit}"),
        RejectionReason::BelowExchangeMinimum { requested, minimum } => {
            format!("valor da ordem {requested} abaixo do mínimo exigido pela corretora {minimum}")
        }
    }
}

fn log_position_event(instrument: &Instrument, event: &PositionEvent) {
    match event {
        PositionEvent::Opened(position) => info!(
            symbol = %instrument.symbol,
            side = ?position.side,
            quantity = %position.quantity,
            entry_price = %position.entry_price,
            "position opened"
        ),
        PositionEvent::Closed(position) => info!(
            symbol = %instrument.symbol,
            pnl_gross = ?position.realized_pnl_gross,
            pnl_net = ?position.realized_pnl_net,
            "position closed"
        ),
    }
}
