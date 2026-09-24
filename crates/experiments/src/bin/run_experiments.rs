use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Parser;
use domain::{Asset, Timeframe};
use experiments::{
    all_factories, default_config, load_candles, run_single_experiment, ExperimentError, WINDOWS,
};
use market_data::binance::BinanceRestClient;
use tracing_subscriber::EnvFilter;

/// Roda cada estratégia (3 baselines + 3 quantitativas) isoladamente
/// contra dados históricos já baixados por `fetch-data`, para cada
/// combinação de símbolo × timeframe × janela pedida. Cada combinação
/// também roda sobre o período completo disponível E sobre os recortes
/// móveis recentes de `experiments::WINDOWS` (24m/12m/6m/3m a partir do
/// candle mais recente) — mesma config, mesmos parâmetros default,
/// nenhum ajuste entre janelas. 100% offline (só uma chamada de rede por
/// símbolo, para metadados de tick/lot/min notional — nunca Postgres,
/// nunca broker live). Escreve um JSON completo por experimento em
/// `results/{symbol}/{timeframe}/{strategy}/{window}.json`, mais um
/// resumo comparativo (`results/summary.md`/`.csv`) no final.
#[derive(Parser, Debug)]
struct Args {
    /// Pares base/quote separados por vírgula. Default: só BTC/USDT (o
    /// pedido inicial) — ETH/USDT e SOL/USDT já têm dados baixados e
    /// podem ser incluídos aqui sem mudar nenhum código.
    #[arg(long, default_value = "BTC/USDT")]
    symbols: String,
    /// Timeframes separados por vírgula (1m, 5m, 15m, 30m, 1h, 4h, 1d, 1w).
    #[arg(long, default_value = "5m,15m,1h")]
    timeframes: String,
    #[arg(long, default_value = "data/historical")]
    data_dir: PathBuf,
    #[arg(long, default_value = "results")]
    results_dir: PathBuf,
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

fn parse_timeframe(raw: &str) -> Result<Timeframe> {
    Ok(match raw.trim() {
        "1m" => Timeframe::M1,
        "5m" => Timeframe::M5,
        "15m" => Timeframe::M15,
        "30m" => Timeframe::M30,
        "1h" => Timeframe::H1,
        "4h" => Timeframe::H4,
        "1d" => Timeframe::D1,
        "1w" => Timeframe::W1,
        other => bail!("unknown timeframe {other:?}; expected one of 1m,5m,15m,30m,1h,4h,1d,1w"),
    })
}

fn parse_timeframes(raw: &str) -> Result<Vec<Timeframe>> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(parse_timeframe)
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
    let timeframes = parse_timeframes(&args.timeframes)?;
    let config = default_config();
    let rest = BinanceRestClient::public();
    let factories = all_factories();

    let mut all_reports = Vec::new();
    // Uma falha de qualidade de dados em modo strict (ver
    // `run_single_experiment`) descarta só aquela combinação
    // símbolo/timeframe/estratégia/janela — não interrompe o lote inteiro,
    // já que janelas diferentes do mesmo símbolo tipicamente têm
    // qualidade de dados diferente (ex.: "full" pode cruzar um gap
    // histórico antigo que uma janela recente de 3m nunca vê). Qualquer
    // outro tipo de erro (registro de estratégia, falha do backtest, I/O)
    // continua abortando o processo — não é algo que faça sentido
    // "pular".
    let mut data_quality_failures: Vec<String> = Vec::new();

    for (base, quote) in &pairs {
        let symbol = historical_data::symbol::folder_symbol(base, quote);
        // Única chamada de rede desta ferramenta: metadados de
        // tick/lot/min notional do instrumento (não candles, não broker
        // live) — o mesmo endpoint que `app::setup::load_instruments` já
        // usa em produção.
        let instrument = rest
            .fetch_instrument(base, quote)
            .await
            .with_context(|| format!("fetching instrument metadata for {symbol}"))?;

        for &timeframe in &timeframes {
            let candles = load_candles(&args.data_dir, &symbol, timeframe, &instrument)
                .with_context(|| format!("loading local candles for {symbol}/{timeframe}"))?;
            tracing::info!(symbol, %timeframe, candles = candles.len(), "loaded historical candles");

            for (kind, factory) in &factories {
                for &(window_label, months) in WINDOWS {
                    let windowed_candles = experiments::slice_recent_window(&candles, months);
                    if windowed_candles.is_empty() {
                        tracing::warn!(
                            symbol, %timeframe, strategy = kind, window = window_label,
                            "skipping: no candles fall inside this window"
                        );
                        continue;
                    }
                    tracing::info!(
                        symbol, %timeframe, strategy = kind, window = window_label,
                        candles = windowed_candles.len(),
                        "running experiment"
                    );
                    let outcome = run_single_experiment(
                        &instrument,
                        &symbol,
                        timeframe,
                        kind,
                        window_label,
                        *factory,
                        windowed_candles,
                        &config,
                        /* strict = */ true,
                    )
                    .await;

                    let report = match outcome {
                        Ok(report) => report,
                        Err(err @ ExperimentError::DataQuality { .. }) => {
                            eprintln!(
                                "{symbol} {timeframe} {kind} [{window_label}]: PULADO (modo \
                                 strict) — {err}"
                            );
                            data_quality_failures.push(err.to_string());
                            continue;
                        }
                        Err(other) => {
                            return Err(other).with_context(|| {
                                format!("running {kind} on {symbol}/{timeframe}/{window_label}")
                            });
                        }
                    };

                    let path = args
                        .results_dir
                        .join(&symbol)
                        .join(timeframe.as_str())
                        .join(kind)
                        .join(format!("{window_label}.json"));
                    report.save_json(&path)?;
                    println!(
                        "{symbol} {timeframe} {kind} [{window}]: {trades} trades, \
                         retorno líquido {net}, win rate {wr}, max DD {dd}, \
                         gaps {gaps} ({missing} candles faltando)",
                        window = window_label,
                        trades = report.total_trades,
                        net = report.net_return_pct,
                        wr = report.win_rate,
                        dd = report.max_drawdown_pct,
                        gaps = report.gap_count,
                        missing = report.missing_candles_total,
                    );
                    all_reports.push(report);
                }
            }
        }
    }

    let summary_md = args.results_dir.join("summary.md");
    let summary_csv = args.results_dir.join("summary.csv");
    experiments::summary::write_markdown_summary(&all_reports, &summary_md)?;
    experiments::summary::write_csv_summary(&all_reports, &summary_csv)?;
    println!(
        "\n{} experimentos concluídos, {} pulados por qualidade de dados (modo strict). \
         Resumo em {} e {}",
        all_reports.len(),
        data_quality_failures.len(),
        summary_md.display(),
        summary_csv.display()
    );
    for failure in &data_quality_failures {
        println!("  pulado: {failure}");
    }

    Ok(())
}
