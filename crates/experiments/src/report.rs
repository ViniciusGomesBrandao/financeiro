//! O relatório serializável de um único experimento (instrumento ×
//! timeframe × estratégia). Achata `analytics::PerformanceReport` +
//! `analytics::EquityCurveReport` + benchmark num único JSON — `analytics`
//! continua sendo o dono do *cálculo*, este módulo só decide *como
//! apresentar/persistir* o resultado (por isso a duplicação de campos: é
//! uma fronteira de serialização, não uma reimplementação de métrica).

use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use domain::Candle;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::ExperimentError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MonthlyReturnDto {
    pub year: i32,
    pub month: u32,
    pub return_pct: Decimal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AnnualReturnDto {
    pub year: i32,
    pub return_pct: Decimal,
}

/// `Deserialize` além de `Serialize` deliberado: `diagnostic::load_all_reports`
/// relê exatamente os JSONs que `save_json` gravou, para o relatório
/// diagnóstico consolidado — nunca reconstrói/reinterpreta os números a
/// partir de outra fonte.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentReport {
    pub symbol: String,
    pub timeframe: String,
    pub strategy_id: String,
    /// Rótulo da janela rodada — `"full"` (histórico completo disponível)
    /// ou `"24m"`/`"12m"`/`"6m"`/`"3m"` (recorte móvel a partir do candle
    /// mais recente, mesma config, zero tuning — ver `crate::window`).
    pub window: String,
    pub period_start: Option<DateTime<Utc>>,
    pub period_end: Option<DateTime<Utc>>,
    pub candles_used: usize,
    pub initial_balance: Decimal,

    // Qualidade dos dados desta execução específica (ver
    // `historical_data::validate`) — contabilizado e reportado em todo
    // experimento, nunca só nos logs de download, e nunca silenciosamente
    // ignorado mesmo quando zero.
    pub gap_count: usize,
    pub missing_candles_total: i64,
    pub duplicate_candles: usize,
    pub out_of_order_candles: usize,

    // De `analytics::PerformanceReport` (sequência de trades fechados).
    pub total_trades: usize,
    pub winners: usize,
    pub losers: usize,
    pub win_rate: Decimal,
    pub gross_pnl: Decimal,
    pub net_pnl: Decimal,
    pub average_win: Decimal,
    pub average_loss: Decimal,
    pub profit_factor: Option<Decimal>,
    /// Drawdown absoluto sobre a sequência de P&L dos trades (não
    /// percentual) — ver `max_drawdown_pct` para a versão percentual sobre
    /// a curva de equity real.
    pub max_drawdown_abs: Decimal,
    pub average_trade_duration_seconds: i64,
    pub total_fees: Decimal,
    pub total_spread_cost: Decimal,
    pub total_slippage_cost: Decimal,

    // De `analytics::EquityCurveReport` (curva de equity no calendário).
    /// Retorno líquido total (equity final vs. saldo inicial).
    pub net_return_pct: Decimal,
    /// Retorno bruto total: `gross_pnl / saldo inicial` — antes das taxas,
    /// mas com spread/slippage já embutidos nos preços de fill (mesma
    /// definição de "bruto" que `domain::Position::realized_pnl_gross` já
    /// usa em todo o resto do projeto).
    pub gross_return_pct: Decimal,
    pub average_daily_return_pct: Decimal,
    pub positive_days: usize,
    pub negative_days: usize,
    pub flat_days: usize,
    pub monthly_returns: Vec<MonthlyReturnDto>,
    pub annual_returns: Vec<AnnualReturnDto>,
    pub max_drawdown_pct: Decimal,

    /// Retorno de comprar-e-segurar o próprio ativo no mesmo período.
    /// `None` só quando não havia candles para calcular (não deveria
    /// acontecer para um experimento que rodou de verdade).
    pub benchmark_return_pct: Option<Decimal>,

    /// Todo trade fechado, em ordem cronológica de fechamento — o
    /// `domain::Position` exatamente como o backtest o produziu (preço de
    /// entrada/saída já com spread/slippage embutidos, quantidade já
    /// normalizada ao lot size do instrumento, fees/spread/slippage
    /// discriminados, P&L bruto e líquido), sem nenhuma transformação.
    /// Preservado para auditoria manual — os totais agregados acima são
    /// derivados disto, não o substituem.
    pub closed_positions: Vec<domain::Position>,

    pub generated_at: DateTime<Utc>,
}

#[allow(clippy::too_many_arguments)]
pub fn build_report(
    symbol: &str,
    timeframe: domain::Timeframe,
    strategy_id: &str,
    window: &str,
    candles: &[Candle],
    initial_balance: Decimal,
    backtest_report: &backtest::BacktestReport,
) -> ExperimentReport {
    let performance = analytics::compute_performance(&backtest_report.closed_positions);
    let equity =
        analytics::compute_equity_curve_report(&backtest_report.equity_curve, initial_balance);
    let benchmark_return_pct = analytics::compute_buy_and_hold_return(candles);
    // Valida exatamente o recorte de candles usado nesta execução (não só
    // o histórico completo baixado) — uma janela de 3m pode não ter gap
    // nenhum mesmo que o histórico completo tenha um, e vice-versa; o
    // relatório de cada experimento precisa refletir os dados que ele
    // realmente rodou, não uma validação genérica feita em outro momento.
    let data_quality = historical_data::validate(candles, timeframe);

    let gross_return_pct = if initial_balance != Decimal::ZERO {
        performance.gross_pnl / initial_balance
    } else {
        Decimal::ZERO
    };

    let mut closed_positions = backtest_report.closed_positions.clone();
    closed_positions.sort_by_key(|p| p.closed_at);

    ExperimentReport {
        symbol: symbol.to_string(),
        timeframe: timeframe.as_str().to_string(),
        strategy_id: strategy_id.to_string(),
        window: window.to_string(),
        period_start: candles.first().map(|c| c.open_time),
        period_end: candles.last().map(|c| c.close_time),
        candles_used: candles.len(),
        initial_balance,

        gap_count: data_quality.gaps.len(),
        missing_candles_total: data_quality.total_missing_candles(),
        duplicate_candles: data_quality.duplicates.len(),
        out_of_order_candles: data_quality.out_of_order.len(),

        total_trades: performance.total_trades,
        winners: performance.winners,
        losers: performance.losers,
        win_rate: performance.win_rate,
        gross_pnl: performance.gross_pnl,
        net_pnl: performance.net_pnl,
        average_win: performance.average_win,
        average_loss: performance.average_loss,
        profit_factor: performance.profit_factor,
        max_drawdown_abs: performance.max_drawdown,
        average_trade_duration_seconds: performance.average_trade_duration.num_seconds(),
        total_fees: performance.total_fees,
        total_spread_cost: performance.total_spread_cost,
        total_slippage_cost: performance.total_slippage_cost,

        net_return_pct: equity.net_return_pct,
        gross_return_pct,
        average_daily_return_pct: equity.average_daily_return_pct,
        positive_days: equity.positive_days,
        negative_days: equity.negative_days,
        flat_days: equity.flat_days,
        monthly_returns: equity
            .monthly_returns
            .iter()
            .map(|m| MonthlyReturnDto {
                year: m.year,
                month: m.month,
                return_pct: m.return_pct,
            })
            .collect(),
        annual_returns: equity
            .annual_returns
            .iter()
            .map(|a| AnnualReturnDto {
                year: a.year,
                return_pct: a.return_pct,
            })
            .collect(),
        max_drawdown_pct: equity.max_drawdown_pct,

        benchmark_return_pct,
        closed_positions,
        generated_at: Utc::now(),
    }
}

impl ExperimentReport {
    /// Grava este relatório como JSON legível em `path`, criando os
    /// diretórios pai se necessário.
    pub fn save_json(&self, path: &Path) -> Result<(), ExperimentError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| ExperimentError::Io {
                path: parent.display().to_string(),
                source,
            })?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json).map_err(|source| ExperimentError::Io {
            path: path.display().to_string(),
            source,
        })
    }
}
