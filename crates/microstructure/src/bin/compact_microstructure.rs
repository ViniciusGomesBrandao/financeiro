//! Compacta as partições Parquet de um símbolo/dia (`microstructure::compact`)
//! — junta os vários arquivos de flush pequenos num único arquivo maior.
//! Ferramenta offline: não rode contra o dia de hoje enquanto o coletor
//! ainda estiver escrevendo nele (ver o doc do módulo `compact`).

use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{NaiveDate, Utc};
use clap::Parser;
use microstructure::compact::compact_day;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
struct Args {
    /// Pares base/quote separados por vírgula.
    #[arg(long, default_value = "BTC/USDT,ETH/USDT,SOL/USDT")]
    symbols: String,
    #[arg(long, default_value = "data/microstructure")]
    data_dir: PathBuf,
    /// Dia UTC a compactar (YYYY-MM-DD). Default: ontem (UTC) — nunca
    /// hoje por default, para não competir com um coletor ainda ativo.
    #[arg(long)]
    date: Option<String>,
}

fn parse_symbols(raw: &str) -> Result<Vec<String>> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|pair| {
            let (base, quote) = pair
                .split_once('/')
                .with_context(|| format!("expected BASE/QUOTE, got {pair:?}"))?;
            Ok(format!("{}{}", base.to_uppercase(), quote.to_uppercase()))
        })
        .collect()
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
        None => (Utc::now() - chrono::Duration::days(1)).date_naive(),
    };
    let symbols = parse_symbols(&args.symbols)?;

    for symbol in symbols {
        let summaries =
            compact_day(&args.data_dir, &symbol, date).context("compacting partitions")?;
        for summary in summaries {
            if summary.skipped_already_compact {
                println!(
                    "{symbol} {date} {}: já compacto ({} arquivo(s)), nada a fazer",
                    summary.kind, summary.files_before
                );
            } else {
                println!(
                    "{symbol} {date} {}: {} arquivos -> {} ({} linhas)",
                    summary.kind, summary.files_before, summary.files_after, summary.rows
                );
            }
        }
    }

    Ok(())
}
