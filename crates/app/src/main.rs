use std::collections::HashMap;

use anyhow::{Context, Result};
use app::{config::AppConfig, pipeline, robot_market, robot_runtime, robot_sync, setup};
use domain::Money;
use execution::{PaperBroker, PaperBrokerConfig};
use market_data::{BinanceMarketData, MarketDataProvider};
use risk::{RiskConfig, RiskEngine};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = AppConfig::from_env().context("loading configuration from environment")?;
    tracing::info!(
        symbols = ?config.enabled_symbols.iter().map(|s| format!("{}/{}", s.base, s.quote)).collect::<Vec<_>>(),
        strategies = ?config.enabled_strategies,
        "quant-engine starting up (paper trading only)"
    );

    let pool = persistence::connect(&config.database_url)
        .await
        .context("connecting to Postgres")?;
    persistence::run_migrations(&pool)
        .await
        .context("running database migrations")?;

    let runtime = setup::resolve_runtime_config(&pool, &config)
        .await
        .context("resolving runtime config from database")?;

    let provider = BinanceMarketData::public();
    let instruments_list = setup::load_instruments(&runtime.symbols, &provider, &pool).await?;
    let mut instrument_index: HashMap<_, _> =
        instruments_list.iter().map(|i| (i.id, i.clone())).collect();

    let capabilities = provider.capabilities();
    let available_market_data: std::collections::HashSet<_> =
        capabilities.market_data_kinds.into_iter().collect();
    let mut registry = setup::build_strategy_registry(
        &runtime,
        &config,
        &instruments_list,
        &available_market_data,
        &pool,
    )
    .await?;

    if registry.is_empty() && runtime.running_robots.is_empty() {
        anyhow::bail!(
            "no strategy was successfully registered against any instrument; check \
             ENABLED_STRATEGIES / ENABLED_SYMBOLS / operational robots and compatibility warnings"
        );
    }

    let mut robots = setup::build_robot_contexts(&runtime, &instruments_list, &config)
        .context("building robot contexts")?;
    // Permite subir o motor sem robôs (só dashboard): hot-reload ativa depois.
    if robots.is_empty() {
        tracing::warn!(
            "no robot contexts at startup; waiting for operational robots via hot-reload"
        );
    }
    for robot in &robots {
        tracing::info!(
            robot_id = %robot.id,
            symbol = %robot.symbol,
            timeframe = %robot.timeframe,
            capital = %robot.paper_capital,
            strategies = ?robot.strategy_ids.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            "robot operational unit ready"
        );
    }

    let risk_engine = RiskEngine::new(RiskConfig {
        order_notional: Money::new(config.risk_order_notional),
        max_position_notional: Money::new(config.risk_max_position_notional),
        max_total_exposure: Money::new(config.risk_max_total_exposure),
        max_open_positions: config.risk_max_open_positions,
        stop_loss_pct: config.risk_stop_loss_pct,
        take_profit_pct: config.risk_take_profit_pct,
        max_daily_loss: Money::new(config.risk_max_daily_loss),
    });

    let mut broker = PaperBroker::new(PaperBrokerConfig {
        maker_fee: config.paper_maker_fee,
        taker_fee: config.paper_taker_fee,
        spread_bps: config.paper_spread_bps,
        slippage_bps: config.paper_slippage_bps,
    });

    let mut portfolios = setup::restore_robot_portfolios(&pool, &robots).await?;
    let mut tracker = setup::build_active_strategy_tracker();
    let mut market_views = robot_market::build_robot_market_views(&robots);

    if !robots.is_empty() {
        pipeline::warm_up_robots(
            &mut registry,
            &provider,
            &instrument_index,
            &robots,
            &mut market_views,
            config.warmup_candles,
        )
        .await?;

        pipeline::seed_judge_after_warmup(
            &instrument_index,
            &robots,
            &mut tracker,
            &portfolios,
            &market_views,
            &pool,
        )
        .await?;
    }

    let (hub, rx) = robot_runtime::MarketEventHub::new();
    let mut stream_keys = robot_sync::initial_stream_keys(&robots);
    let groups = robot_runtime::group_instruments_by_timeframe(&robots, &instruments_list);
    for (timeframe, group_instruments) in groups {
        tracing::info!(
            timeframe = %timeframe,
            symbols = ?group_instruments.iter().map(|i| i.symbol.to_string()).collect::<Vec<_>>(),
            "opening market-data stream"
        );
        let recv = provider.stream(group_instruments, timeframe).await?;
        hub.attach(recv);
    }

    let hot_reload = robot_sync::HotReload {
        provider: &provider,
        hub: &hub,
        stream_keys: &mut stream_keys,
        available_market_data: available_market_data.clone(),
        warmup_candles: config.warmup_candles,
        config: &config,
    };

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::info!("Ctrl+C received, shutting down gracefully");
            let _ = shutdown_tx.send(());
        }
    });

    pipeline::run(
        rx,
        &mut instrument_index,
        &mut robots,
        &mut registry,
        &mut tracker,
        &risk_engine,
        &mut broker,
        &mut portfolios,
        &mut market_views,
        &pool,
        Some(hot_reload),
        shutdown_rx,
    )
    .await?;

    for (robot_id, portfolio) in &portfolios {
        let report = analytics::compute_performance(portfolio.closed_positions());
        tracing::info!(
            robot_id = %robot_id,
            total_trades = report.total_trades,
            win_rate = %report.win_rate,
            net_pnl = %report.net_pnl,
            "robot session performance summary"
        );
    }

    Ok(())
}
