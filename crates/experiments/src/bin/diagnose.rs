//! Lê todos os `ExperimentReport` já gravados sob `results/` e escreve o
//! relatório diagnóstico consolidado (`results/diagnostic.json`): uma
//! linha por combinação com os campos pedidos + classificação mecânica,
//! mais a comparação baseline/quantitativa e a consistência entre janelas.

use std::path::PathBuf;

use clap::Parser;
use experiments::diagnostic::{
    build_row, compute_consistency, compute_pair_comparisons, load_all_reports, ConsistencyEntry,
    DiagnosticRow, PairComparison,
};
use serde::Serialize;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = "results")]
    results_dir: PathBuf,
    #[arg(long, default_value = "results/diagnostic.json")]
    out: PathBuf,
}

#[derive(Serialize)]
struct Diagnostic {
    generated_at: chrono::DateTime<chrono::Utc>,
    rows: Vec<DiagnosticRow>,
    consistency: Vec<ConsistencyEntry>,
    pair_comparisons: Vec<PairComparison>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let reports = load_all_reports(&args.results_dir)?;
    println!(
        "{} relatórios carregados de {}",
        reports.len(),
        args.results_dir.display()
    );

    let rows: Vec<DiagnosticRow> = reports.iter().map(build_row).collect();
    let consistency = compute_consistency(&rows);
    let pair_comparisons = compute_pair_comparisons(&rows);

    let inconsistent = consistency.iter().filter(|c| !c.consistent).count();
    println!(
        "{} combinações símbolo/timeframe/estratégia; {} inconsistentes entre janelas",
        consistency.len(),
        inconsistent
    );
    let diff_pairs = pair_comparisons
        .iter()
        .filter(|p| !p.same_classification)
        .count();
    println!(
        "{} comparações baseline/quantitativa; {} com classificação diferente entre as duas",
        pair_comparisons.len(),
        diff_pairs
    );

    let diagnostic = Diagnostic {
        generated_at: chrono::Utc::now(),
        rows,
        consistency,
        pair_comparisons,
    };
    let json = serde_json::to_string_pretty(&diagnostic)?;
    std::fs::write(&args.out, json)?;
    println!("gravado em {}", args.out.display());

    Ok(())
}
