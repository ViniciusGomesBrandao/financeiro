use std::collections::HashMap;

use domain::{InstrumentId, PortfolioSnapshot, Position};
use features::FeatureSnapshot;

/// Resultado de uma execução de backtest: o estado final do portfólio e
/// todo trade fechado durante a reprodução. Deliberadamente não calcula
/// métricas de performance por conta própria — para isso, passe
/// `closed_positions` para `analytics::compute_performance`, exatamente
/// como o loop live/da aplicação faria com
/// `portfolio::PortfolioManager::closed_positions()`.
#[derive(Debug, Clone)]
pub struct BacktestReport {
    pub final_snapshot: PortfolioSnapshot,
    pub closed_positions: Vec<Position>,
    pub candles_processed: usize,
    /// Um `FeatureSnapshot` por candle fechado processado, por instrumento,
    /// na mesma ordem cronológica da reprodução — o snapshot no índice `i`
    /// reflete só os candles até ali (ver `features::compute_series` e o
    /// doc do crate `features` para a garantia de não-look-ahead). As
    /// estratégias atuais (`strategies::generic::*`) não consomem isto
    /// ainda — calculado aqui para permitir análise/pesquisa sobre o
    /// histórico do backtest, e para uma futura migração de estratégias
    /// para consumir features em vez de recalcular indicadores.
    pub feature_snapshots: HashMap<InstrumentId, Vec<FeatureSnapshot>>,
    /// Um `PortfolioSnapshot` por candle fechado processado, em ordem
    /// cronológica — a série de equity ao longo de toda a reprodução.
    /// Passe para `analytics::compute_equity_curve_report` para retorno
    /// diário/mensal/anual e drawdown baseado em equity; `analytics`
    /// permanece a única dona do cálculo de métricas de performance, este
    /// campo só entrega a matéria-prima.
    pub equity_curve: Vec<PortfolioSnapshot>,
}
