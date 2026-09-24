//! Lê o que `collect-microstructure` já persistiu para um símbolo/dia,
//! reconstrói o book (via `microstructure::compute_series`, que já valida
//! a sequência de deltas) e roda o `MicrostructureFeatureEngine`, imprimindo
//! um resumo — a prova de ponta a ponta de que persistência ->
//! reconstrução -> features fecha sobre dado real, não só sintético.

use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{NaiveDate, Utc};
use clap::Parser;
use microstructure::replay::load_day_events;
use microstructure::MicrostructureConfig;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
struct Args {
    /// Símbolo no formato de transmissão, ex.: BTCUSDT.
    #[arg(long)]
    symbol: String,
    #[arg(long, default_value = "data/microstructure")]
    data_dir: PathBuf,
    /// Dia UTC a reprocessar (YYYY-MM-DD). Default: hoje (UTC).
    #[arg(long)]
    date: Option<String>,
}

fn fmt_opt(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.6}"))
        .unwrap_or_else(|| "None".to_string())
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
        "{}: {} eventos carregados para {date}",
        args.symbol,
        events.len()
    );
    if events.is_empty() {
        println!("nada para reprocessar (nenhum dado persistido para este símbolo/dia)");
        return Ok(());
    }

    let instrument_id = microstructure::deterministic_instrument_id(&args.symbol);
    let snapshots =
        microstructure::compute_series(instrument_id, &events, MicrostructureConfig::default())
            .context(
                "failed to reconstruct book / compute features — see error for the exact gap",
            )?;

    println!(
        "{} MicrostructureSnapshot gerados (um por atualização de book)",
        snapshots.len()
    );
    if let Some(last) = snapshots.last() {
        println!("último snapshot ({}):", last.timestamp);
        println!("  spread_abs         = {}", fmt_opt(last.spread_abs));
        println!("  spread_pct         = {}", fmt_opt(last.spread_pct));
        println!("  mid_price          = {}", fmt_opt(last.mid_price));
        println!("  microprice         = {}", fmt_opt(last.microprice));
        println!("  bid_ask_imbalance  = {}", fmt_opt(last.bid_ask_imbalance));
        println!("  book_imbalance     = {}", fmt_opt(last.book_imbalance));
        println!("  trade_imbalance    = {}", fmt_opt(last.trade_imbalance));
        println!("  volume_delta       = {}", fmt_opt(last.volume_delta));
        println!("  trade_intensity    = {}", fmt_opt(last.trade_intensity));
        println!(
            "  order_flow_imbalance = {}",
            fmt_opt(last.order_flow_imbalance)
        );
    }

    let spreads: Vec<f64> = snapshots.iter().filter_map(|s| s.spread_pct).collect();
    if !spreads.is_empty() {
        let avg = spreads.iter().sum::<f64>() / spreads.len() as f64;
        println!(
            "spread_pct médio no dia: {avg:.6} ({} leituras)",
            spreads.len()
        );
    }

    Ok(())
}
