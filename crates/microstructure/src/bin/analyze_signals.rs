//! Roda `microstructure::analyzer::analyze` sobre a grade de features já
//! construída (`build-feature-grid`) de um ou mais símbolos, e grava um
//! relatório JSON consolidado — a entrada para o relatório de pesquisa
//! (dashboard). Puramente diagnóstico: não escolhe parâmetro nenhum, não
//! decide estratégia nenhuma.

use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{NaiveDate, Utc};
use clap::Parser;
use microstructure::analyzer::{analyze, estimate_round_trip_cost_pct, DEFAULT_HORIZONS_SECS};
use microstructure::store;
use serde::Serialize;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = "BTCUSDT,ETHUSDT,SOLUSDT")]
    symbols: String,
    #[arg(long, default_value = "data/microstructure")]
    data_dir: PathBuf,
    /// Dia UTC a analisar (YYYY-MM-DD). Default: hoje (UTC).
    #[arg(long)]
    date: Option<String>,
    #[arg(long, default_value = "results/microstructure_signal_report.json")]
    out: PathBuf,
}

#[derive(Serialize)]
struct SymbolReport {
    symbol: String,
    n_grid_points: usize,
    round_trip_cost_pct: f64,
    stats: Vec<microstructure::analyzer::FeatureHorizonStats>,
}

#[derive(Serialize)]
struct Report {
    generated_at: chrono::DateTime<Utc>,
    date: NaiveDate,
    horizons_secs: Vec<i64>,
    symbols: Vec<SymbolReport>,
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

    let symbols: Vec<String> = args
        .symbols
        .split(',')
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())
        .collect();

    let mut symbol_reports = Vec::new();
    for symbol in &symbols {
        let rows = store::read_feature_grid_day(&args.data_dir, symbol, date)
            .with_context(|| format!("reading feature grid for {symbol}"))?;
        if rows.is_empty() {
            println!("{symbol}: nenhum ponto de grade para {date}, pulando");
            continue;
        }
        let round_trip_cost_pct = estimate_round_trip_cost_pct(&rows);
        let stats = analyze(&rows, &DEFAULT_HORIZONS_SECS);

        println!(
            "{symbol}: {} pontos de grade, custo round-trip estimado = {:.5}%",
            rows.len(),
            round_trip_cost_pct * 100.0
        );
        for s in &stats {
            let corr = s
                .pearson_correlation
                .map(|c| format!("{c:.4}"))
                .unwrap_or_else(|| "None".to_string());
            let ci = match (s.pearson_ci95_lo, s.pearson_ci95_hi) {
                (Some(lo), Some(hi)) => format!("[{lo:.4},{hi:.4}]"),
                _ => "[n/a]".to_string(),
            };
            let hit = s
                .hit_rate
                .map(|h| format!("{:.1}%", h * 100.0))
                .unwrap_or_else(|| "N/A".to_string());
            println!(
                "  {:<22} {:>4}s  n={:>6}  n_eff={:>5}  pearson={:>8} ci95={:<18}  hit_rate={:>7}  edge>cost={:?}",
                s.feature, s.horizon_secs, s.n, s.n_effective, corr, ci, hit, s.edge_exceeds_cost
            );
        }

        symbol_reports.push(SymbolReport {
            symbol: symbol.clone(),
            n_grid_points: rows.len(),
            round_trip_cost_pct,
            stats,
        });
    }

    let report = Report {
        generated_at: Utc::now(),
        date,
        horizons_secs: DEFAULT_HORIZONS_SECS.to_vec(),
        symbols: symbol_reports,
    };

    if let Some(parent) = args.out.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let json = serde_json::to_string_pretty(&report)?;
    std::fs::write(&args.out, json).with_context(|| format!("writing {}", args.out.display()))?;
    println!("relatório gravado em {}", args.out.display());

    Ok(())
}
