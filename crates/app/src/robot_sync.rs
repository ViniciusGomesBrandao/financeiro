//! Hot-load / hot-stop de robôs operacionais sem reiniciar o `quant-engine`.
//!
//! A cada tick o pipeline consulta `operational_robots` com status `running`
//! e:
//! - ativa robôs novos (registry + portfolio + warmup + stream se preciso);
//! - desativa robôs parados / legado quando o modo DB está ativo.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use domain::{Instrument, InstrumentId, MarketDataKind, StrategyId, Timeframe};
use market_data::{BinanceMarketData, MarketDataProvider};
use portfolio::PortfolioManager;
use sqlx::PgPool;
use strategies::StrategyRegistry;
use tracing::{info, warn};

use crate::config::AppConfig;
use crate::config::SymbolPair;
use crate::pipeline;
use crate::robot_market::{self, RobotMarketView};
use crate::robot_runtime::{MarketEventHub, RobotContext};
use crate::setup;
use crate::strategy_switch::ActiveStrategyTracker;

/// Chave de assinatura de stream: timeframe + símbolo.
pub type StreamKey = (Timeframe, String);

pub struct HotReload<'a> {
    pub provider: &'a BinanceMarketData,
    pub hub: &'a MarketEventHub,
    pub stream_keys: &'a mut HashSet<StreamKey>,
    pub available_market_data: HashSet<MarketDataKind>,
    pub warmup_candles: u32,
    pub config: &'a AppConfig,
}

/// Sincroniza a lista ao vivo com o Postgres.
pub async fn sync_from_database(
    hot: &mut HotReload<'_>,
    instruments: &mut HashMap<InstrumentId, Instrument>,
    robots: &mut Vec<RobotContext>,
    registry: &mut StrategyRegistry,
    tracker: &mut ActiveStrategyTracker,
    portfolios: &mut HashMap<String, PortfolioManager>,
    market_views: &mut HashMap<String, RobotMarketView>,
    pool: &PgPool,
) -> Result<()> {
    let running = persistence::operational_robots::list_running(pool).await?;

    if running.is_empty() {
        // Sem robôs operacionais: mantém o que já estiver (legado). Não
        // remove nada automaticamente — o modo legado é só na subida.
        return Ok(());
    }

    let desired_ids: HashSet<String> = running.iter().map(|r| r.id.clone()).collect();

    // Sai do modo legado: robôs sintéticos `legacy-*` não competem com os
    // operacionais do dashboard.
    let removed_legacy: Vec<String> = robots
        .iter()
        .filter(|r| r.id.starts_with("legacy-") || !desired_ids.contains(&r.id))
        .map(|r| r.id.clone())
        .collect();
    if !removed_legacy.is_empty() {
        robots.retain(|r| desired_ids.contains(&r.id));
        for id in &removed_legacy {
            if id.starts_with("legacy-") {
                info!(robot_id = %id, "hot-reload: deactivated legacy robot (operational mode)");
            } else {
                info!(robot_id = %id, "hot-reload: robot stopped or removed");
            }
        }
    }

    let active_ids: HashSet<String> = robots.iter().map(|r| r.id.clone()).collect();

    for op in &running {
        if active_ids.contains(&op.id) {
            continue;
        }
        match activate_robot(hot, op, instruments, robots, registry, tracker, portfolios, market_views, pool)
            .await
        {
            Ok(()) => info!(
                robot_id = %op.id,
                symbol = %op.symbol,
                timeframe = %op.timeframe,
                "hot-reload: robot activated"
            ),
            Err(err) => warn!(
                robot_id = %op.id,
                error = %err,
                "hot-reload: failed to activate robot"
            ),
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn activate_robot(
    hot: &mut HotReload<'_>,
    op: &persistence::operational_robots::OperationalRobot,
    instruments: &mut HashMap<InstrumentId, Instrument>,
    robots: &mut Vec<RobotContext>,
    registry: &mut StrategyRegistry,
    tracker: &mut ActiveStrategyTracker,
    portfolios: &mut HashMap<String, PortfolioManager>,
    market_views: &mut HashMap<String, RobotMarketView>,
    pool: &PgPool,
) -> Result<()> {
    let instrument = ensure_instrument(hot.provider, instruments, pool, &op.symbol).await?;
    let timeframe = Timeframe::parse(&op.timeframe).with_context(|| {
        format!(
            "robot {} has invalid timeframe {:?}",
            op.id, op.timeframe
        )
    })?;

    let strategy_ids: Vec<StrategyId> = op
        .candidate_kinds
        .iter()
        .map(|kind| {
            StrategyId::new(persistence::operational_robots::strategy_instance_id(
                &op.id, kind,
            ))
            .with_context(|| format!("invalid instance id for robot {}", op.id))
        })
        .collect::<Result<Vec<_>>>()?;

    let instance_configs: Vec<strategies::StrategyInstanceConfig> = op
        .candidate_kinds
        .iter()
        .map(|kind| {
            let id = StrategyId::new(persistence::operational_robots::strategy_instance_id(
                &op.id, kind,
            ))?;
            Ok(strategies::StrategyInstanceConfig {
                id,
                kind: kind.clone(),
                symbols: vec![op.symbol.clone()],
            })
        })
        .collect::<Result<Vec<_>>>()?;

    // Garante strategy_configs enabled + registro no registry.
    for instance in &instance_configs {
        let descriptor = strategies::catalog::get(&instance.kind)
            .with_context(|| format!("unknown strategy kind {:?}", instance.kind))?
            .descriptor;
        let record = persistence::strategy_configs::StrategyConfigRecord {
            id: instance.id.clone(),
            strategy_kind: instance.kind.clone(),
            params: descriptor.default_params,
            supported_asset_classes: descriptor.requirements.supported_asset_classes.clone(),
            required_market_data: descriptor.requirements.required_market_data.clone(),
            enabled: true,
        };
        persistence::strategy_configs::upsert(pool, &record).await?;
    }

    let instrument_list: Vec<Instrument> = instruments.values().cloned().collect();
    strategies::register_instances(
        registry,
        &instance_configs,
        &instrument_list,
        &hot.available_market_data,
    )
    .context("registering strategy instances for hot-loaded robot")?;

    let robot = RobotContext {
        id: op.id.clone(),
        instrument_id: instrument.id,
        symbol: op.symbol.clone(),
        timeframe,
        strategy_ids,
        paper_capital: op.paper_capital,
    };

    let restored = setup::restore_robot_portfolios(pool, &[robot.clone()]).await?;
    for (id, pm) in restored {
        portfolios.insert(id, pm);
    }
    market_views.insert(
        robot.id.clone(),
        robot_market::RobotMarketView::new(robot.instrument_id),
    );

    ensure_stream(hot, &instrument, timeframe).await?;

    pipeline::warm_up_robots(
        registry,
        hot.provider,
        instruments,
        &[robot.clone()],
        market_views,
        hot.warmup_candles,
    )
    .await?;

    pipeline::seed_judge_after_warmup(
        instruments,
        &[robot.clone()],
        tracker,
        portfolios,
        market_views,
        pool,
    )
    .await?;

    robots.push(robot);
    Ok(())
}

async fn ensure_instrument(
    provider: &BinanceMarketData,
    instruments: &mut HashMap<InstrumentId, Instrument>,
    pool: &PgPool,
    symbol: &str,
) -> Result<Instrument> {
    if let Some(existing) = instruments.values().find(|i| i.symbol.as_str() == symbol) {
        return Ok(existing.clone());
    }

    let (base, quote) = symbol.split_once('/').with_context(|| {
        format!("invalid symbol {symbol:?}, expected BASE/QUOTE")
    })?;
    let pairs = [SymbolPair {
        base: base.to_string(),
        quote: quote.to_string(),
    }];
    // Reusa o loader oficial (upsert no Postgres + id estável).
    let loaded = setup::load_instruments(&pairs, provider, pool).await?;
    let instrument = loaded
        .into_iter()
        .next()
        .with_context(|| format!("failed to load instrument {symbol}"))?;
    instruments.insert(instrument.id, instrument.clone());
    Ok(instrument)
}

async fn ensure_stream(
    hot: &mut HotReload<'_>,
    instrument: &Instrument,
    timeframe: Timeframe,
) -> Result<()> {
    let key = (timeframe, instrument.symbol.to_string());
    if hot.stream_keys.contains(&key) {
        return Ok(());
    }
    info!(
        symbol = %instrument.symbol,
        timeframe = %timeframe,
        "hot-reload: opening market-data stream"
    );
    let recv = hot
        .provider
        .stream(vec![instrument.clone()], timeframe)
        .await
        .context("opening hot-reload market stream")?;
    hot.hub.attach(recv);
    hot.stream_keys.insert(key);
    Ok(())
}

/// Constrói o conjunto inicial de chaves de stream a partir dos robôs.
pub fn initial_stream_keys(robots: &[RobotContext]) -> HashSet<StreamKey> {
    robots
        .iter()
        .map(|r| (r.timeframe, r.symbol.clone()))
        .collect()
}
