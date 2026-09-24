//! Métricas de performance derivadas da curva de equity ao longo do tempo
//! (`domain::PortfolioSnapshot`, uma amostra por candle processado — ver
//! `backtest::BacktestReport::equity_curve`), em vez da sequência de trades
//! fechados que `performance::compute_performance` usa. As duas visões se
//! complementam: uma mede "o que cada trade rendeu", a outra "como o
//! patrimônio evoluiu no calendário" (retorno diário/mensal/anual,
//! drawdown percentual sobre a curva real de equity).
//!
//! **Dia = dia-calendário UTC, deliberadamente, não "dia útil".** Cripto
//! negocia 24 horas por dia, 7 dias por semana, sem feriado nem fechamento
//! de bolsa — um sábado ou domingo é um dia de retorno como qualquer
//! outro, não algo a pular ou tratar como "sem pregão". `downsample para
//! diário` abaixo usa só `DateTime::date_naive()` (o calendário UTC puro),
//! sem nenhuma noção de sessão de negociação, dia útil ou calendário de
//! feriados — ver `no_days_are_skipped_for_weekends_crypto_trades_24_7`.

use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};
use domain::PortfolioSnapshot;
use rust_decimal::Decimal;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonthlyReturn {
    pub year: i32,
    pub month: u32,
    pub return_pct: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnnualReturn {
    pub year: i32,
    pub return_pct: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EquityCurveReport {
    /// Retorno líquido total do período — igual ao `return_pct` do último
    /// snapshot (equity final vs. saldo inicial).
    pub net_return_pct: Decimal,
    /// Média simples dos retornos diários de equity (fim de dia UTC contra
    /// fim do dia anterior; o primeiro dia é contra o saldo inicial).
    pub average_daily_return_pct: Decimal,
    pub positive_days: usize,
    pub negative_days: usize,
    pub flat_days: usize,
    /// Um item por mês-calendário (UTC) coberto pela curva, em ordem
    /// cronológica.
    pub monthly_returns: Vec<MonthlyReturn>,
    /// Um item por ano-calendário (UTC) coberto pela curva, em ordem
    /// cronológica.
    pub annual_returns: Vec<AnnualReturn>,
    /// Drawdown percentual de pico a vale sobre a curva de equity inteira
    /// (não downsampled para diário) — complementa, sem substituir, o
    /// `PerformanceReport::max_drawdown` absoluto baseado em sequência de
    /// trades.
    pub max_drawdown_pct: Decimal,
}

fn zeroed_report() -> EquityCurveReport {
    EquityCurveReport {
        net_return_pct: Decimal::ZERO,
        average_daily_return_pct: Decimal::ZERO,
        positive_days: 0,
        negative_days: 0,
        flat_days: 0,
        monthly_returns: Vec::new(),
        annual_returns: Vec::new(),
        max_drawdown_pct: Decimal::ZERO,
    }
}

/// Calcula `EquityCurveReport` a partir de `snapshots` (deve vir em ordem
/// cronológica — a ordem de `backtest::BacktestReport::equity_curve` já
/// garante isso) e `initial_cash` (o saldo antes do primeiro candle,
/// usado como referência do primeiríssimo retorno diário/mensal/anual).
/// `snapshots` vazio produz um relatório zerado, não um erro.
pub fn compute_equity_curve_report(
    snapshots: &[PortfolioSnapshot],
    initial_cash: Decimal,
) -> EquityCurveReport {
    let Some(last) = snapshots.last() else {
        return zeroed_report();
    };

    let max_drawdown_pct = max_drawdown_pct(snapshots);

    // Downsample para 1 ponto por dia-calendário UTC (o último snapshot
    // daquele dia) — uma `BTreeMap` mantém os dias em ordem cronológica de
    // graça, e sobrescrever a cada snapshot do mesmo dia naturalmente fica
    // com o último (os snapshots chegam em ordem cronológica).
    let mut daily_equity: BTreeMap<NaiveDate, Decimal> = BTreeMap::new();
    for snapshot in snapshots {
        daily_equity.insert(snapshot.timestamp.date_naive(), snapshot.equity);
    }

    let mut daily_returns = Vec::with_capacity(daily_equity.len());
    let mut positive_days = 0usize;
    let mut negative_days = 0usize;
    let mut flat_days = 0usize;
    let mut prev_equity = initial_cash;
    for &equity in daily_equity.values() {
        let daily_return = period_return(prev_equity, equity);
        daily_returns.push(daily_return);
        match daily_return.cmp(&Decimal::ZERO) {
            std::cmp::Ordering::Greater => positive_days += 1,
            std::cmp::Ordering::Less => negative_days += 1,
            std::cmp::Ordering::Equal => flat_days += 1,
        }
        prev_equity = equity;
    }
    let average_daily_return_pct = if daily_returns.is_empty() {
        Decimal::ZERO
    } else {
        daily_returns.iter().sum::<Decimal>() / Decimal::from(daily_returns.len() as u64)
    };

    let monthly_returns = period_returns(&daily_equity, |d| (d.year(), d.month()), initial_cash)
        .into_iter()
        .map(|((year, month), return_pct)| MonthlyReturn {
            year,
            month,
            return_pct,
        })
        .collect();
    let annual_returns = period_returns(&daily_equity, |d| d.year(), initial_cash)
        .into_iter()
        .map(|(year, return_pct)| AnnualReturn { year, return_pct })
        .collect();

    EquityCurveReport {
        net_return_pct: last.return_pct,
        average_daily_return_pct,
        positive_days,
        negative_days,
        flat_days,
        monthly_returns,
        annual_returns,
        max_drawdown_pct,
    }
}

fn period_return(prev_equity: Decimal, equity: Decimal) -> Decimal {
    if prev_equity == Decimal::ZERO {
        Decimal::ZERO
    } else {
        (equity - prev_equity) / prev_equity
    }
}

/// Agrupa `daily_equity` por `key_fn` (mês ou ano), tomando o equity de
/// fim-de-período (o último dia observado daquele período) e comparando
/// contra o fim do período anterior (ou `initial_cash` para o primeiro
/// período da série) — a mesma definição de retorno por período tanto para
/// mês quanto para ano, parametrizada só pela chave de agrupamento.
fn period_returns<K: Ord + Copy>(
    daily_equity: &BTreeMap<NaiveDate, Decimal>,
    key_fn: impl Fn(NaiveDate) -> K,
    initial_cash: Decimal,
) -> Vec<(K, Decimal)> {
    let mut period_end_equity: BTreeMap<K, Decimal> = BTreeMap::new();
    for (&date, &equity) in daily_equity {
        period_end_equity.insert(key_fn(date), equity);
    }

    let mut result = Vec::with_capacity(period_end_equity.len());
    let mut prev_equity = initial_cash;
    for (key, equity) in period_end_equity {
        result.push((key, period_return(prev_equity, equity)));
        prev_equity = equity;
    }
    result
}

fn max_drawdown_pct(snapshots: &[PortfolioSnapshot]) -> Decimal {
    let mut peak = Decimal::ZERO;
    let mut max_drawdown = Decimal::ZERO;
    for snapshot in snapshots {
        peak = peak.max(snapshot.equity);
        if peak > Decimal::ZERO {
            let drawdown = (peak - snapshot.equity) / peak;
            max_drawdown = max_drawdown.max(drawdown);
        }
    }
    max_drawdown
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use rust_decimal_macros::dec;

    fn snapshot(day: u32, hour: u32, equity: Decimal) -> PortfolioSnapshot {
        let timestamp = Utc.with_ymd_and_hms(2024, 1, day, hour, 0, 0).unwrap();
        PortfolioSnapshot {
            timestamp,
            cash: equity,
            equity,
            realized_pnl: equity - dec!(1000),
            unrealized_pnl: Decimal::ZERO,
            open_positions_count: 0,
            exposure_ratio: Decimal::ZERO,
            return_pct: (equity - dec!(1000)) / dec!(1000),
            realized_pnl_today: Decimal::ZERO,
        }
    }

    #[test]
    fn empty_snapshots_produce_zeroed_report() {
        let report = compute_equity_curve_report(&[], dec!(1000));
        assert_eq!(report.net_return_pct, Decimal::ZERO);
        assert!(report.monthly_returns.is_empty());
        assert!(report.annual_returns.is_empty());
    }

    #[test]
    fn downsamples_to_last_snapshot_per_utc_day_and_counts_positive_negative_days() {
        let snapshots = vec![
            snapshot(1, 0, dec!(1000)),
            snapshot(1, 12, dec!(1100)), // dia 1 fecha em 1100 (só o último conta)
            snapshot(2, 0, dec!(1050)),  // dia 2: queda
            snapshot(3, 0, dec!(1200)),  // dia 3: alta
        ];
        let report = compute_equity_curve_report(&snapshots, dec!(1000));

        // dia1: 1000->1100 (+10%), dia2: 1100->1050 (-4.5...%), dia3: 1050->1200 (+14.28...%)
        assert_eq!(report.positive_days, 2);
        assert_eq!(report.negative_days, 1);
        assert_eq!(report.flat_days, 0);
        assert_eq!(report.net_return_pct, dec!(0.2)); // último snapshot: (1200-1000)/1000
    }

    #[test]
    fn max_drawdown_pct_uses_the_full_curve_not_the_daily_downsample() {
        // Sobe pra 1200, cai pra 600 no meio do MESMO dia (intraday), sobe
        // de volta pra 1100 antes do fim do dia — se drawdown fosse
        // calculado só sobre o downsample diário (que só vê 1000 e 1100),
        // o mergulho intraday para 600 (-50% do pico) desapareceria.
        let snapshots = vec![
            snapshot(1, 0, dec!(1200)),
            snapshot(1, 6, dec!(600)),
            snapshot(1, 12, dec!(1100)),
        ];
        let report = compute_equity_curve_report(&snapshots, dec!(1000));
        assert_eq!(report.max_drawdown_pct, dec!(0.5)); // (1200-600)/1200
    }

    #[test]
    fn monthly_and_annual_returns_use_end_of_period_equity() {
        let snapshots = vec![
            snapshot(1, 0, dec!(1100)),  // fim de janeiro (nesta série)
            snapshot(31, 0, dec!(1300)), // ainda janeiro -> substitui como fim de mês
        ];
        let report = compute_equity_curve_report(&snapshots, dec!(1000));

        assert_eq!(report.monthly_returns.len(), 1);
        assert_eq!(report.monthly_returns[0].year, 2024);
        assert_eq!(report.monthly_returns[0].month, 1);
        assert_eq!(report.monthly_returns[0].return_pct, dec!(0.3)); // (1300-1000)/1000

        assert_eq!(report.annual_returns.len(), 1);
        assert_eq!(report.annual_returns[0].year, 2024);
        assert_eq!(report.annual_returns[0].return_pct, dec!(0.3));
    }

    #[test]
    fn no_days_are_skipped_for_weekends_crypto_trades_24_7() {
        // 2024-01-05 é sexta, 06 é sábado, 07 é domingo, 08 é segunda.
        // Cripto não fecha no fim de semana — os 4 dias precisam contar
        // igualmente, sábado e domingo incluídos, sem nenhum tratamento de
        // "dia útil"/sessão de negociação.
        let snapshots = vec![
            snapshot(5, 0, dec!(1000)), // sexta: igual ao saldo inicial -> retorno 0 (flat)
            snapshot(6, 0, dec!(1100)), // sábado: +10% (conta como dia normal)
            snapshot(7, 0, dec!(1050)), // domingo: -4.5...% (conta como dia normal)
            snapshot(8, 0, dec!(1200)), // segunda: +14.28...%
        ];
        let report = compute_equity_curve_report(&snapshots, dec!(1000));

        // 4 dias observados no total (positive + negative + flat), nenhum
        // pulado por cair em fim de semana.
        assert_eq!(
            report.positive_days + report.negative_days + report.flat_days,
            4
        );
        assert_eq!(report.positive_days, 2); // sábado, segunda
        assert_eq!(report.negative_days, 1); // domingo
        assert_eq!(report.flat_days, 1); // sexta (igual ao saldo inicial)
    }
}
