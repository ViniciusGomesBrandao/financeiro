//! Resumo comparativo de todos os experimentos rodados numa mesma
//! invocação — uma tabela Markdown (para leitura humana) e um CSV
//! equivalente (para análise em planilha/script), lado a lado com o JSON
//! completo de cada experimento (`report::ExperimentReport::save_json`)
//! que preserva todo o detalhe para auditoria.

use std::fs;
use std::path::Path;

use rust_decimal::Decimal;

use crate::error::ExperimentError;
use crate::report::ExperimentReport;

fn pct(value: Decimal) -> String {
    format!("{:.2}%", value * Decimal::from(100))
}

fn opt_pct(value: Option<Decimal>) -> String {
    value.map(pct).unwrap_or_else(|| "-".to_string())
}

fn opt_decimal(value: Option<Decimal>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn write_file(path: &Path, contents: &str) -> Result<(), ExperimentError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ExperimentError::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    fs::write(path, contents).map_err(|source| ExperimentError::Io {
        path: path.display().to_string(),
        source,
    })
}

pub fn write_markdown_summary(
    reports: &[ExperimentReport],
    path: &Path,
) -> Result<(), ExperimentError> {
    let mut out = String::new();
    out.push_str("# Resumo comparativo de experimentos\n\n");
    out.push_str(&format!(
        "Gerado em {} — {} execuções.\n\n",
        chrono::Utc::now().to_rfc3339(),
        reports.len()
    ));
    out.push_str(
        "| Símbolo | Timeframe | Janela | Estratégia | Candles | Gaps (candles faltando) | Trades | Win rate | Profit factor | Retorno líquido | Retorno bruto | Retorno médio/dia | Max DD (equity) | Benchmark |\n",
    );
    out.push_str("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n");
    for r in reports {
        let gaps_summary = format!("{} ({})", r.gap_count, r.missing_candles_total);
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            r.symbol,
            r.timeframe,
            r.window,
            r.strategy_id,
            r.candles_used,
            gaps_summary,
            r.total_trades,
            pct(r.win_rate),
            opt_decimal(r.profit_factor),
            pct(r.net_return_pct),
            pct(r.gross_return_pct),
            pct(r.average_daily_return_pct),
            pct(r.max_drawdown_pct),
            opt_pct(r.benchmark_return_pct),
        ));
    }
    write_file(path, &out)
}

pub fn write_csv_summary(reports: &[ExperimentReport], path: &Path) -> Result<(), ExperimentError> {
    let mut out = String::new();
    out.push_str(
        "symbol,timeframe,window,strategy_id,candles_used,gap_count,missing_candles_total,\
         duplicate_candles,out_of_order_candles,total_trades,winners,losers,win_rate,\
         profit_factor,net_return_pct,gross_return_pct,average_daily_return_pct,\
         positive_days,negative_days,max_drawdown_pct,max_drawdown_abs,average_win,\
         average_loss,average_trade_duration_seconds,total_fees,total_spread_cost,\
         total_slippage_cost,benchmark_return_pct\n",
    );
    for r in reports {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            r.symbol,
            r.timeframe,
            r.window,
            r.strategy_id,
            r.candles_used,
            r.gap_count,
            r.missing_candles_total,
            r.duplicate_candles,
            r.out_of_order_candles,
            r.total_trades,
            r.winners,
            r.losers,
            r.win_rate,
            opt_decimal(r.profit_factor),
            r.net_return_pct,
            r.gross_return_pct,
            r.average_daily_return_pct,
            r.positive_days,
            r.negative_days,
            r.max_drawdown_pct,
            r.max_drawdown_abs,
            r.average_win,
            r.average_loss,
            r.average_trade_duration_seconds,
            r.total_fees,
            r.total_spread_cost,
            r.total_slippage_cost,
            opt_decimal(r.benchmark_return_pct),
        ));
    }
    write_file(path, &out)
}
