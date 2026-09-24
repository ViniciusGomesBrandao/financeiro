//! Constrói a grade determinística de `MicrostructureSnapshot` (default:
//! 1s) de um símbolo/dia a partir do que já está persistido
//! (`replay::load_day_events` + `grid::snapshot_grid`) e grava em
//! `store::FeatureGridRow` — a entrada que `analyze-signals` consome.

use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{Duration, NaiveDate, Utc};
use clap::Parser;
use microstructure::replay::load_day_events;
use microstructure::store::{self, FeatureGridRow};
use microstructure::{snapshot_grid, MicrostructureConfig};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    symbol: String,
    #[arg(long, default_value = "data/microstructure")]
    data_dir: PathBuf,
    /// Dia UTC a processar (YYYY-MM-DD). Default: hoje (UTC).
    #[arg(long)]
    date: Option<String>,
    /// Espaçamento da grade, em segundos.
    #[arg(long, default_value_t = 1)]
    interval_secs: i64,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let date = match &args.date {
        Some(raw) => NaiveDate::parse_from_str(raw, "%Y-%m-%d")
            .context("invalid --date, expected YYYY-MM-DD")?,
        None => Utc::now().date_naive(),
    };

    let events = load_day_events(&args.data_dir, &args.symbol, date)
        .context("failed to load persisted microstructure data")?;
    println!(
        "{}: {} eventos brutos carregados para {date}",
        args.symbol,
        events.len()
    );
    if events.is_empty() {
        println!("nada para processar (nenhum dado persistido para este símbolo/dia)");
        return Ok(());
    }

    let instrument_id = microstructure::deterministic_instrument_id(&args.symbol);
    let interval = Duration::seconds(args.interval_secs);
    let snapshots = snapshot_grid(
        instrument_id,
        &events,
        MicrostructureConfig::default(),
        interval,
    )
    .context("failed to build the snapshot grid — see error for the exact gap")?;

    println!(
        "{} pontos de grade gerados (a cada {}s)",
        snapshots.len(),
        args.interval_secs
    );
    if snapshots.is_empty() {
        return Ok(());
    }

    let rows: Vec<FeatureGridRow> = snapshots
        .iter()
        .map(FeatureGridRow::from_snapshot)
        .collect();
    let dir = store::day_dir(&args.data_dir, &args.symbol, "feature_grid", date);
    store::write_feature_grid(&dir, Utc::now(), &rows).context("writing feature grid")?;

    let with_mid = rows.iter().filter(|r| r.mid_price.is_some()).count();
    let with_trade_data = rows.iter().filter(|r| r.trade_imbalance.is_some()).count();
    println!(
        "gravado em {} — {with_mid}/{} pontos com mid_price, {with_trade_data}/{} com dados de trade tape",
        dir.display(),
        rows.len(),
        rows.len()
    );

    Ok(())
}
