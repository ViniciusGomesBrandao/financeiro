use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use domain::Asset;
use historical_data::update_pair;
use market_data::binance::BinanceRestClient;
use tracing_subscriber::EnvFilter;

/// Baixa/atualiza incrementalmente o histórico local de candles de 1m para
/// os símbolos informados, e regenera os timeframes derivados (5m/15m/1h).
/// Idempotente: rodar de novo só baixa o que ainda não existe localmente.
#[derive(Parser, Debug)]
struct Args {
    /// Pares base/quote separados por vírgula, ex.: BTC/USDT,ETH/USDT.
    #[arg(long, default_value = "BTC/USDT,ETH/USDT,SOL/USDT")]
    symbols: String,
    /// Diretório onde os arquivos Parquet são lidos/gravados.
    #[arg(long, default_value = "data/historical")]
    data_dir: PathBuf,
}

fn parse_symbols(raw: &str) -> Result<Vec<(Asset, Asset)>> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|pair| {
            let (base, quote) = pair
                .split_once('/')
                .with_context(|| format!("expected BASE/QUOTE, got {pair:?}"))?;
            Ok((Asset::new(base)?, Asset::new(quote)?))
        })
        .collect()
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let pairs = parse_symbols(&args.symbols)?;
    let rest = BinanceRestClient::public();

    for (base, quote) in pairs {
        let symbol = historical_data::symbol::folder_symbol(&base, &quote);
        tracing::info!(symbol, "starting update");
        let summary = update_pair(&rest, &args.data_dir, &base, &quote, &symbol).await?;

        let validation_path =
            historical_data::validation_report_path(&args.data_dir, &symbol, domain::Timeframe::M1);
        println!(
            "{symbol}: {new} novos candles de 1m ({before} -> {after} total); \
             período {earliest:?} .. {latest:?}; validação: {oo} fora de ordem, \
             {dup} duplicatas, {gaps} gaps ({missing} candles faltando no total) — \
             relatório completo em {validation_path}",
            symbol = summary.symbol,
            new = summary.new_candles,
            before = summary.candles_before,
            after = summary.candles_after,
            earliest = summary.earliest,
            latest = summary.latest,
            oo = summary.validation.out_of_order.len(),
            dup = summary.validation.duplicates.len(),
            gaps = summary.validation.gaps.len(),
            missing = summary.validation.total_missing_candles(),
            validation_path = validation_path.display(),
        );
        for gap in &summary.validation.gaps {
            println!(
                "  gap: {} candles faltando entre {} e {}",
                gap.missing_candles, gap.after, gap.before
            );
        }
    }

    Ok(())
}
