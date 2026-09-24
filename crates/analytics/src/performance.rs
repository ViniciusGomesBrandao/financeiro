use chrono::Duration;
use domain::{Position, PositionStatus};
use rust_decimal::Decimal;

/// Métricas de performance a nível de trade calculadas a partir de um
/// conjunto de posições fechadas. Todo número aqui é derivado, nunca
/// inventado — uma estratégia sem trades fechados ainda produz um relatório
/// de zeros / `None`, e não um valor de placeholder fabricado.
#[derive(Debug, Clone, PartialEq)]
pub struct PerformanceReport {
    pub total_trades: usize,
    pub winners: usize,
    pub losers: usize,
    /// Fração de trades com P&L líquido positivo, ex.: `0.55` = 55%.
    pub win_rate: Decimal,
    pub gross_pnl: Decimal,
    pub net_pnl: Decimal,
    pub average_win: Decimal,
    pub average_loss: Decimal,
    /// Soma do P&L dos trades vencedores dividida pela soma do P&L dos
    /// perdedores (em magnitude). `None` quando não há trades perdedores
    /// para servir de divisor, em vez de reportar um infinito enganoso.
    pub profit_factor: Option<Decimal>,
    /// Ganho médio esperado por trade: `win_rate * average_win -
    /// (1 - win_rate) * average_loss`. Positivo significa que a
    /// estratégia tem edge estatístico na amostra observada; `Decimal::ZERO`
    /// quando não há trades. Construído inteiramente a partir de
    /// `win_rate`/`average_win`/`average_loss` já calculados acima — não é
    /// uma métrica independente.
    pub expectancy: Decimal,
    /// Maior drawdown de pico a vale do *P&L líquido acumulado ao longo dos
    /// trades fechados, em ordem cronológica* — não é um drawdown real de
    /// curva de equity (isso exigiria também os efeitos de caixa/alavancagem
    /// entre trades, que o histórico de snapshots do
    /// `portfolio::PortfolioManager` captura com mais precisão depois de
    /// persistido). Trate isto como uma aproximação baseada na sequência de
    /// trades.
    pub max_drawdown: Decimal,
    /// Duração média entre `opened_at` e `closed_at` dos trades fechados.
    /// `Duration::zero()` quando não há trades.
    pub average_trade_duration: Duration,
    /// Soma de `Position::fees_paid` (entrada + saída) de todos os trades
    /// fechados.
    pub total_fees: Decimal,
    /// Soma de `Position::spread_paid` — só diagnóstico, já refletido nos
    /// preços de entrada/saída (ver o doc de `Position::spread_paid`).
    pub total_spread_cost: Decimal,
    /// Soma de `Position::slippage_paid` — mesma ressalva de
    /// `total_spread_cost`.
    pub total_slippage_cost: Decimal,
}

/// Calcula um `PerformanceReport` a partir de `positions`, considerando
/// apenas aquelas com `status == PositionStatus::Closed` (posições abertas
/// são ignoradas, e não tratadas como break-even).
pub fn compute_performance(positions: &[Position]) -> PerformanceReport {
    let mut closed: Vec<&Position> = positions
        .iter()
        .filter(|p| p.status == PositionStatus::Closed)
        .collect();
    closed.sort_by_key(|p| p.closed_at);

    let total_trades = closed.len();
    let mut winners = 0usize;
    let mut losers = 0usize;
    let mut gross_pnl = Decimal::ZERO;
    let mut net_pnl = Decimal::ZERO;
    let mut sum_win = Decimal::ZERO;
    let mut sum_loss_magnitude = Decimal::ZERO;
    let mut cumulative = Decimal::ZERO;
    let mut peak = Decimal::ZERO;
    let mut max_drawdown = Decimal::ZERO;
    let mut total_fees = Decimal::ZERO;
    let mut total_spread_cost = Decimal::ZERO;
    let mut total_slippage_cost = Decimal::ZERO;
    let mut total_duration = Duration::zero();

    for position in &closed {
        let net = position.realized_pnl_net.unwrap_or(Decimal::ZERO);
        let gross = position.realized_pnl_gross.unwrap_or(Decimal::ZERO);
        gross_pnl += gross;
        net_pnl += net;

        if net > Decimal::ZERO {
            winners += 1;
            sum_win += net;
        } else if net < Decimal::ZERO {
            losers += 1;
            sum_loss_magnitude += -net;
        }

        cumulative += net;
        peak = peak.max(cumulative);
        max_drawdown = max_drawdown.max(peak - cumulative);

        total_fees += position.fees_paid;
        total_spread_cost += position.spread_paid;
        total_slippage_cost += position.slippage_paid;
        if let Some(closed_at) = position.closed_at {
            total_duration += closed_at - position.opened_at;
        }
    }

    let average_trade_duration = if total_trades > 0 {
        total_duration / total_trades as i32
    } else {
        Duration::zero()
    };

    let win_rate = if total_trades > 0 {
        Decimal::from(winners as u64) / Decimal::from(total_trades as u64)
    } else {
        Decimal::ZERO
    };
    let average_win = if winners > 0 {
        sum_win / Decimal::from(winners as u64)
    } else {
        Decimal::ZERO
    };
    let average_loss = if losers > 0 {
        sum_loss_magnitude / Decimal::from(losers as u64)
    } else {
        Decimal::ZERO
    };
    let profit_factor = if sum_loss_magnitude > Decimal::ZERO {
        Some(sum_win / sum_loss_magnitude)
    } else {
        None
    };
    let expectancy = if total_trades > 0 {
        win_rate * average_win - (Decimal::ONE - win_rate) * average_loss
    } else {
        Decimal::ZERO
    };

    PerformanceReport {
        total_trades,
        winners,
        losers,
        win_rate,
        gross_pnl,
        net_pnl,
        average_win,
        average_loss,
        profit_factor,
        expectancy,
        max_drawdown,
        average_trade_duration,
        total_fees,
        total_spread_cost,
        total_slippage_cost,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use domain::{InstrumentId, Side, StrategyId};
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn closed_position_with_costs(
        net_pnl: Decimal,
        closed_offset_secs: i64,
        fees: Decimal,
        spread: Decimal,
        slippage: Decimal,
    ) -> Position {
        let mut position = closed_position(net_pnl, closed_offset_secs);
        position.fees_paid = fees;
        position.spread_paid = spread;
        position.slippage_paid = slippage;
        position
    }

    fn closed_position(net_pnl: Decimal, closed_offset_secs: i64) -> Position {
        let now = Utc::now();
        Position {
            id: Uuid::new_v4(),
            instrument_id: InstrumentId::new(),
            strategy_id: StrategyId::new("test").unwrap(),
            side: Side::Buy,
            quantity: dec!(1),
            entry_price: dec!(100),
            exit_price: Some(dec!(100) + net_pnl),
            opened_at: now,
            closed_at: Some(now + Duration::seconds(closed_offset_secs)),
            status: PositionStatus::Closed,
            realized_pnl_gross: Some(net_pnl),
            realized_pnl_net: Some(net_pnl),
            fees_paid: Decimal::ZERO,
            spread_paid: Decimal::ZERO,
            slippage_paid: Decimal::ZERO,
        }
    }

    #[test]
    fn empty_input_produces_zeroed_report() {
        let report = compute_performance(&[]);
        assert_eq!(report.total_trades, 0);
        assert_eq!(report.win_rate, Decimal::ZERO);
        assert_eq!(report.profit_factor, None);
    }

    #[test]
    fn computes_win_rate_and_pnl_totals() {
        let positions = vec![
            closed_position(dec!(100), 1),
            closed_position(dec!(-50), 2),
            closed_position(dec!(200), 3),
            closed_position(dec!(-25), 4),
        ];
        let report = compute_performance(&positions);

        assert_eq!(report.total_trades, 4);
        assert_eq!(report.winners, 2);
        assert_eq!(report.losers, 2);
        assert_eq!(report.win_rate, dec!(0.5));
        assert_eq!(report.net_pnl, dec!(225));
        assert_eq!(report.average_win, dec!(150));
        assert_eq!(report.average_loss, dec!(37.5));
        // profit factor = 300 / 75 = 4
        assert_eq!(report.profit_factor, Some(dec!(4)));
    }

    #[test]
    fn ignores_open_positions() {
        let mut open = closed_position(dec!(999), 1);
        open.status = PositionStatus::Open;
        open.realized_pnl_net = None;
        let report = compute_performance(&[open]);
        assert_eq!(report.total_trades, 0);
    }

    #[test]
    fn max_drawdown_tracks_peak_to_trough_of_cumulative_pnl() {
        // Sequência acumulada: 100, 150 (pico), 50 (drawdown 100), 120.
        let positions = vec![
            closed_position(dec!(100), 1),
            closed_position(dec!(50), 2),
            closed_position(dec!(-100), 3),
            closed_position(dec!(70), 4),
        ];
        let report = compute_performance(&positions);
        assert_eq!(report.max_drawdown, dec!(100));
    }

    #[test]
    fn expectancy_is_zero_with_no_trades() {
        let report = compute_performance(&[]);
        assert_eq!(report.expectancy, Decimal::ZERO);
    }

    #[test]
    fn expectancy_matches_win_rate_weighted_formula() {
        // 2 vencedores de 100, 2 perdedores de 50 -> win_rate=0.5,
        // average_win=100, average_loss=50.
        // expectancy = 0.5*100 - 0.5*50 = 50 - 25 = 25.
        let positions = vec![
            closed_position(dec!(100), 1),
            closed_position(dec!(-50), 2),
            closed_position(dec!(100), 3),
            closed_position(dec!(-50), 4),
        ];
        let report = compute_performance(&positions);
        assert_eq!(report.expectancy, dec!(25));
    }

    #[test]
    fn negative_expectancy_reflects_a_losing_edge() {
        // 1 vencedor de 10, 3 perdedores de 20 -> win_rate=0.25,
        // average_win=10, average_loss=20.
        // expectancy = 0.25*10 - 0.75*20 = 2.5 - 15 = -12.5.
        let positions = vec![
            closed_position(dec!(10), 1),
            closed_position(dec!(-20), 2),
            closed_position(dec!(-20), 3),
            closed_position(dec!(-20), 4),
        ];
        let report = compute_performance(&positions);
        assert_eq!(report.expectancy, dec!(-12.5));
    }

    #[test]
    fn no_losers_means_no_profit_factor() {
        let positions = vec![closed_position(dec!(50), 1), closed_position(dec!(25), 2)];
        let report = compute_performance(&positions);
        assert_eq!(report.profit_factor, None);
    }

    #[test]
    fn averages_trade_duration_and_sums_fees_spread_slippage() {
        // `closed_offset_secs` em `closed_position_with_costs` já é a
        // duração do trade (opened_at = now, closed_at = now + offset).
        let a = closed_position_with_costs(dec!(10), 100, dec!(1), dec!(0.5), dec!(0.2));
        let b = closed_position_with_costs(dec!(20), 300, dec!(2), dec!(1.5), dec!(0.8));

        let report = compute_performance(&[a, b]);

        assert_eq!(report.average_trade_duration, Duration::seconds(200));
        assert_eq!(report.total_fees, dec!(3));
        assert_eq!(report.total_spread_cost, dec!(2));
        assert_eq!(report.total_slippage_cost, dec!(1));
    }

    #[test]
    fn zero_trades_yields_zero_duration() {
        let report = compute_performance(&[]);
        assert_eq!(report.average_trade_duration, Duration::zero());
        assert_eq!(report.total_fees, Decimal::ZERO);
    }
}
