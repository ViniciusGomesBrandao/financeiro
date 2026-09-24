//! Analytics de performance calculadas a partir de posições fechadas e da
//! curva de equity ao longo do tempo.
//!
//! Depende apenas de `domain` — analytics é uma função pura do histórico de
//! trades/equity, sem conhecimento de como esse histórico foi produzido
//! (live vs. backtest) ou armazenado.
//!
//! Duas visões complementares:
//! - `performance` — por sequência de trades fechados: P&L bruto/líquido,
//!   win rate, ganho/perda médios, profit factor, expectancy, duração
//!   média, fees/spread/slippage totais, e uma aproximação do max
//!   drawdown baseada na sequência de trades (`PerformanceReport`).
//! - `equity_curve` — pela curva de equity no calendário
//!   (`domain::PortfolioSnapshot`, tipicamente uma amostra por candle
//!   processado): retorno líquido, retorno diário médio, dias
//!   positivos/negativos, retorno mensal/anual, e drawdown percentual
//!   sobre a curva real de equity (`EquityCurveReport`).
//!
//! `benchmark` calcula o retorno de comprar-e-segurar o próprio ativo no
//! mesmo período, para servir de piso de comparação.
//!
//! Deliberadamente ainda não implementados (não invente estes valores —
//! adicione-os apenas quando houver um cálculo real por trás): índice de
//! Sharpe, índice de Sortino, volatilidade realizada, exposição ponderada
//! pelo tempo, correlação entre estratégias.

pub mod benchmark;
pub mod equity_curve;
pub mod performance;

pub use benchmark::compute_buy_and_hold_return;
pub use equity_curve::{
    compute_equity_curve_report, AnnualReturn, EquityCurveReport, MonthlyReturn,
};
pub use performance::{compute_performance, PerformanceReport};
