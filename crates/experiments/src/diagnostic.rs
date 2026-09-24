//! Relatório diagnóstico consolidado: lê todos os `ExperimentReport` já
//! gravados em disco (nunca recomputa nada a partir de outra fonte) e
//! deriva, por combinação símbolo/timeframe/janela/estratégia, os campos e
//! a classificação pedidos — sem nenhum ajuste de parâmetro/estratégia
//! envolvido, puramente uma leitura + classificação mecânica do que o
//! backtest já produziu.

use std::path::{Path, PathBuf};

use rust_decimal::Decimal;
use serde::Serialize;

use crate::error::ExperimentError;
use crate::report::ExperimentReport;

/// Abaixo deste número de trades fechados, o sinal do retorno (bruto ou
/// líquido) não é uma amostra confiável — poucos trades podem ser
/// positivos ou negativos por acaso. 30 é o mínimo de amostra costumeiro
/// para aproximações estatísticas básicas (regra prática, não derivada
/// dos dados) — escolha documentada aqui, não pedida pelo usuário nem
/// ajustada a nenhum resultado específico.
pub const LOW_SAMPLE_THRESHOLD: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Classification {
    /// `gross_return_pct <= 0`: a estratégia não tinha vantagem nem antes
    /// dos custos de negociação.
    NoEdge,
    /// `gross_return_pct > 0` mas `net_return_pct <= 0`: havia uma
    /// vantagem bruta, mas fees/spread/slippage a consumiram inteira.
    FrictionLimited,
    /// `net_return_pct > 0`: sobrou retorno positivo mesmo depois dos
    /// custos.
    NetPositive,
    /// Menos de `LOW_SAMPLE_THRESHOLD` trades fechados — amostra pequena
    /// demais para que o sinal do retorno signifique algo. Checado antes
    /// das outras três categorias, e as substitui quando aplicável.
    LowSample,
}

impl Classification {
    pub fn label(&self) -> &'static str {
        match self {
            Classification::NoEdge => "NO_EDGE",
            Classification::FrictionLimited => "FRICTION_LIMITED",
            Classification::NetPositive => "NET_POSITIVE",
            Classification::LowSample => "LOW_SAMPLE",
        }
    }
}

/// Classifica `report` conforme as quatro categorias pedidas — mecânico,
/// sem nenhum grau de liberdade além do limiar documentado de
/// `LOW_SAMPLE_THRESHOLD`.
pub fn classify(report: &ExperimentReport) -> Classification {
    if report.total_trades < LOW_SAMPLE_THRESHOLD {
        Classification::LowSample
    } else if report.gross_return_pct <= Decimal::ZERO {
        Classification::NoEdge
    } else if report.net_return_pct <= Decimal::ZERO {
        Classification::FrictionLimited
    } else {
        Classification::NetPositive
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticRow {
    pub symbol: String,
    pub timeframe: String,
    pub window: String,
    pub strategy_id: String,
    pub candles_used: usize,
    pub gross_return_pct: Decimal,
    /// `total_fees + total_spread_cost + total_slippage_cost` — o custo
    /// total de negociação simulado, nas três parcelas que
    /// `execution::PaperBroker` já discrimina.
    pub total_costs: Decimal,
    /// `total_costs / initial_balance` — mesma base (saldo inicial) que
    /// `gross_return_pct`/`net_return_pct` já usam, para que os três
    /// números sejam diretamente comparáveis.
    pub costs_pct: Decimal,
    pub net_return_pct: Decimal,
    pub average_daily_return_pct: Decimal,
    pub total_trades: usize,
    pub win_rate: Decimal,
    pub profit_factor: Option<Decimal>,
    pub max_drawdown_pct: Decimal,
    pub average_win: Decimal,
    pub average_loss: Decimal,
    pub positive_days: usize,
    pub negative_days: usize,
    pub classification: Classification,
}

pub fn build_row(report: &ExperimentReport) -> DiagnosticRow {
    let total_costs = report.total_fees + report.total_spread_cost + report.total_slippage_cost;
    let costs_pct = if report.initial_balance != Decimal::ZERO {
        total_costs / report.initial_balance
    } else {
        Decimal::ZERO
    };

    DiagnosticRow {
        symbol: report.symbol.clone(),
        timeframe: report.timeframe.clone(),
        window: report.window.clone(),
        strategy_id: report.strategy_id.clone(),
        candles_used: report.candles_used,
        gross_return_pct: report.gross_return_pct,
        total_costs,
        costs_pct,
        net_return_pct: report.net_return_pct,
        average_daily_return_pct: report.average_daily_return_pct,
        total_trades: report.total_trades,
        win_rate: report.win_rate,
        profit_factor: report.profit_factor,
        max_drawdown_pct: report.max_drawdown_pct,
        average_win: report.average_win,
        average_loss: report.average_loss,
        positive_days: report.positive_days,
        negative_days: report.negative_days,
        classification: classify(report),
    }
}

/// Lê recursivamente todo `.json` sob `results_dir` (ignora
/// `summary.md`/`summary.csv`/`diagnostic.json`, que não são
/// `ExperimentReport`s) e desserializa cada um. Nunca recomputa/deriva —
/// exatamente os relatórios que `ExperimentReport::save_json` gravou.
pub fn load_all_reports(results_dir: &Path) -> Result<Vec<ExperimentReport>, ExperimentError> {
    let mut reports = Vec::new();
    let mut stack = vec![results_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|source| ExperimentError::Io {
            path: dir.display().to_string(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| ExperimentError::Io {
                path: dir.display().to_string(),
                source,
            })?;
            let path: PathBuf = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            // `diagnostic.json` (a saída deste próprio módulo) tem um
            // formato diferente de `ExperimentReport` — precisa ser
            // excluído explicitamente pelo nome, não só pela extensão,
            // para que rodar `diagnose` de novo sobre o mesmo
            // `results_dir` não tente reler sua própria saída anterior.
            if path.file_name().and_then(|n| n.to_str()) == Some("diagnostic.json") {
                continue;
            }
            let raw = std::fs::read_to_string(&path).map_err(|source| ExperimentError::Io {
                path: path.display().to_string(),
                source,
            })?;
            let report: ExperimentReport = serde_json::from_str(&raw)?;
            reports.push(report);
        }
    }
    Ok(reports)
}

#[derive(Debug, Clone, Serialize)]
pub struct ConsistencyEntry {
    pub symbol: String,
    pub timeframe: String,
    pub strategy_id: String,
    /// Janelas presentes, na ordem `24m, 12m, 6m, 3m` (janelas ausentes —
    /// ex. por terem caído em `LOW_SAMPLE`/skip em algum outro estágio —
    /// simplesmente não aparecem, não viram um buraco na lista).
    pub windows_present: Vec<String>,
    /// Classificação de cada janela, alinhada posicionalmente com
    /// `windows_present`.
    pub classifications: Vec<&'static str>,
    /// `true` se a classificação é a mesma em todas as janelas presentes
    /// (com 0 ou 1 janela, vacuamente consistente).
    pub consistent: bool,
}

const WINDOW_ORDER: [&str; 4] = ["24m", "12m", "6m", "3m"];

/// Agrupa `rows` por (símbolo, timeframe, estratégia) e verifica se a
/// classificação se mantém igual através das janelas 24m/12m/6m/3m — um
/// sinal de robustez (ou falta dela) que nenhuma janela isolada revela
/// sozinha.
pub fn compute_consistency(rows: &[DiagnosticRow]) -> Vec<ConsistencyEntry> {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<(String, String, String), Vec<(String, Classification)>> =
        BTreeMap::new();
    for row in rows {
        groups
            .entry((
                row.symbol.clone(),
                row.timeframe.clone(),
                row.strategy_id.clone(),
            ))
            .or_default()
            .push((row.window.clone(), row.classification));
    }

    groups
        .into_iter()
        .map(|((symbol, timeframe, strategy_id), mut entries)| {
            entries.sort_by_key(|(w, _)| {
                WINDOW_ORDER
                    .iter()
                    .position(|x| x == w)
                    .unwrap_or(usize::MAX)
            });
            let windows_present = entries.iter().map(|(w, _)| w.clone()).collect::<Vec<_>>();
            let classifications = entries.iter().map(|(_, c)| c.label()).collect::<Vec<_>>();
            let consistent = classifications.windows(2).all(|w| w[0] == w[1]);
            ConsistencyEntry {
                symbol,
                timeframe,
                strategy_id,
                windows_present,
                classifications,
                consistent,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct PairComparison {
    pub symbol: String,
    pub timeframe: String,
    pub window: String,
    pub baseline_strategy: String,
    pub quant_strategy: String,
    pub baseline_classification: &'static str,
    pub quant_classification: &'static str,
    pub baseline_net_return_pct: Decimal,
    pub quant_net_return_pct: Decimal,
    pub same_classification: bool,
}

/// Para cada combinação (símbolo, timeframe, janela) em que *ambos* os
/// lados de um par baseline/quantitativa (`STRATEGY_PAIRS`) rodaram, monta
/// a comparação lado a lado. Combinações em que um dos dois lados não
/// aparece em `rows` (ex. caiu em `strict` por gap) são simplesmente
/// omitidas, não preenchidas com um valor inventado.
pub fn compute_pair_comparisons(rows: &[DiagnosticRow]) -> Vec<PairComparison> {
    use std::collections::BTreeMap;
    let index: BTreeMap<(&str, &str, &str, &str), &DiagnosticRow> = rows
        .iter()
        .map(|r| {
            (
                (
                    r.symbol.as_str(),
                    r.timeframe.as_str(),
                    r.window.as_str(),
                    r.strategy_id.as_str(),
                ),
                r,
            )
        })
        .collect();

    let mut combos: Vec<(&str, &str, &str)> = rows
        .iter()
        .map(|r| (r.symbol.as_str(), r.timeframe.as_str(), r.window.as_str()))
        .collect();
    combos.sort();
    combos.dedup();

    let mut result = Vec::new();
    for (symbol, timeframe, window) in combos {
        for (baseline, quant) in STRATEGY_PAIRS {
            let b = index.get(&(symbol, timeframe, window, baseline));
            let q = index.get(&(symbol, timeframe, window, quant));
            if let (Some(b), Some(q)) = (b, q) {
                result.push(PairComparison {
                    symbol: symbol.to_string(),
                    timeframe: timeframe.to_string(),
                    window: window.to_string(),
                    baseline_strategy: baseline.to_string(),
                    quant_strategy: quant.to_string(),
                    baseline_classification: b.classification.label(),
                    quant_classification: q.classification.label(),
                    baseline_net_return_pct: b.net_return_pct,
                    quant_net_return_pct: q.net_return_pct,
                    same_classification: b.classification == q.classification,
                });
            }
        }
    }
    result
}

/// As três duplas baseline/quantitativa, pela hipótese que cada
/// estratégia quantitativa declara compartilhar com sua baseline (ver o
/// doc do módulo de cada `strategies::generic::*`): `momentum` ↔
/// `quant_momentum` e `mean_reversion` ↔ `statistical_mean_reversion` são
/// explicitados nos próprios doc comments ("a mesma premissa de...");
/// `ema_crossover` ↔ `volatility_breakout` é o par restante por
/// eliminação (ambas seguem tendência/rompimento, em vez de reverter à
/// média) — não uma correspondência declarada tão explicitamente quanto
/// as outras duas, sinalizado aqui para quem for interpretar a
/// comparação.
pub const STRATEGY_PAIRS: [(&str, &str); 3] = [
    ("momentum", "quant_momentum"),
    ("mean_reversion", "statistical_mean_reversion"),
    ("ema_crossover", "volatility_breakout"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn report_with(total_trades: usize, gross: Decimal, net: Decimal) -> ExperimentReport {
        // Só os campos que `classify`/`build_row` de fato leem importam
        // aqui; o resto recebe valores neutros.
        ExperimentReport {
            symbol: "TEST".to_string(),
            timeframe: "1h".to_string(),
            strategy_id: "test".to_string(),
            window: "3m".to_string(),
            period_start: None,
            period_end: None,
            candles_used: 0,
            initial_balance: Decimal::new(100_000, 0),
            gap_count: 0,
            missing_candles_total: 0,
            duplicate_candles: 0,
            out_of_order_candles: 0,
            total_trades,
            winners: 0,
            losers: 0,
            win_rate: Decimal::ZERO,
            gross_pnl: Decimal::ZERO,
            net_pnl: Decimal::ZERO,
            average_win: Decimal::ZERO,
            average_loss: Decimal::ZERO,
            profit_factor: None,
            max_drawdown_abs: Decimal::ZERO,
            average_trade_duration_seconds: 0,
            total_fees: Decimal::ZERO,
            total_spread_cost: Decimal::ZERO,
            total_slippage_cost: Decimal::ZERO,
            net_return_pct: net,
            gross_return_pct: gross,
            average_daily_return_pct: Decimal::ZERO,
            positive_days: 0,
            negative_days: 0,
            flat_days: 0,
            monthly_returns: Vec::new(),
            annual_returns: Vec::new(),
            max_drawdown_pct: Decimal::ZERO,
            benchmark_return_pct: None,
            closed_positions: Vec::new(),
            generated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn classifies_low_sample_regardless_of_return_sign() {
        let r = report_with(5, Decimal::new(10, 2), Decimal::new(10, 2));
        assert_eq!(classify(&r), Classification::LowSample);
    }

    #[test]
    fn classifies_no_edge_when_gross_non_positive() {
        let r = report_with(50, Decimal::ZERO, Decimal::ZERO);
        assert_eq!(classify(&r), Classification::NoEdge);
        let r = report_with(50, Decimal::new(-5, 2), Decimal::new(-10, 2));
        assert_eq!(classify(&r), Classification::NoEdge);
    }

    #[test]
    fn classifies_friction_limited_when_gross_positive_but_net_non_positive() {
        let r = report_with(50, Decimal::new(5, 2), Decimal::ZERO);
        assert_eq!(classify(&r), Classification::FrictionLimited);
        let r = report_with(50, Decimal::new(5, 2), Decimal::new(-1, 2));
        assert_eq!(classify(&r), Classification::FrictionLimited);
    }

    #[test]
    fn classifies_net_positive_when_net_positive() {
        let r = report_with(50, Decimal::new(5, 2), Decimal::new(1, 2));
        assert_eq!(classify(&r), Classification::NetPositive);
    }

    #[test]
    fn costs_pct_sums_all_three_cost_components_over_initial_balance() {
        let mut r = report_with(50, Decimal::ONE, Decimal::ONE);
        r.total_fees = Decimal::new(500, 0);
        r.total_spread_cost = Decimal::new(200, 0);
        r.total_slippage_cost = Decimal::new(300, 0);
        r.initial_balance = Decimal::new(100_000, 0);
        let row = build_row(&r);
        assert_eq!(row.total_costs, Decimal::new(1_000, 0));
        assert_eq!(row.costs_pct, Decimal::new(1, 2)); // 1000/100000 = 0.01
    }

    fn row_with(
        symbol: &str,
        timeframe: &str,
        window: &str,
        strategy_id: &str,
        classification: Classification,
    ) -> DiagnosticRow {
        DiagnosticRow {
            symbol: symbol.to_string(),
            timeframe: timeframe.to_string(),
            window: window.to_string(),
            strategy_id: strategy_id.to_string(),
            candles_used: 0,
            gross_return_pct: Decimal::ZERO,
            total_costs: Decimal::ZERO,
            costs_pct: Decimal::ZERO,
            net_return_pct: Decimal::ZERO,
            average_daily_return_pct: Decimal::ZERO,
            total_trades: 0,
            win_rate: Decimal::ZERO,
            profit_factor: None,
            max_drawdown_pct: Decimal::ZERO,
            average_win: Decimal::ZERO,
            average_loss: Decimal::ZERO,
            positive_days: 0,
            negative_days: 0,
            classification,
        }
    }

    #[test]
    fn consistency_flags_a_strategy_that_flips_classification_across_windows() {
        let rows = vec![
            row_with("BTCUSDT", "1h", "24m", "momentum", Classification::NoEdge),
            row_with("BTCUSDT", "1h", "12m", "momentum", Classification::NoEdge),
            row_with(
                "BTCUSDT",
                "1h",
                "6m",
                "momentum",
                Classification::FrictionLimited,
            ),
            row_with("BTCUSDT", "1h", "3m", "momentum", Classification::NoEdge),
        ];
        let consistency = compute_consistency(&rows);
        assert_eq!(consistency.len(), 1);
        assert_eq!(
            consistency[0].windows_present,
            vec!["24m", "12m", "6m", "3m"]
        );
        assert!(!consistency[0].consistent);
    }

    #[test]
    fn consistency_is_true_when_classification_never_changes() {
        let rows = vec![
            row_with("BTCUSDT", "1h", "24m", "momentum", Classification::NoEdge),
            row_with("BTCUSDT", "1h", "3m", "momentum", Classification::NoEdge),
        ];
        let consistency = compute_consistency(&rows);
        assert!(consistency[0].consistent);
    }

    #[test]
    fn pair_comparison_only_includes_combos_where_both_sides_exist() {
        let rows = vec![
            row_with("BTCUSDT", "1h", "3m", "momentum", Classification::NoEdge),
            row_with(
                "BTCUSDT",
                "1h",
                "3m",
                "quant_momentum",
                Classification::FrictionLimited,
            ),
            // ema_crossover sem par volatility_breakout nesta janela -> não deve aparecer.
            row_with(
                "BTCUSDT",
                "1h",
                "3m",
                "ema_crossover",
                Classification::NoEdge,
            ),
        ];
        let pairs = compute_pair_comparisons(&rows);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].baseline_strategy, "momentum");
        assert_eq!(pairs[0].quant_strategy, "quant_momentum");
        assert!(!pairs[0].same_classification);
    }

    #[test]
    fn classification_serializes_as_screaming_snake_case_everywhere() {
        // `DiagnosticRow.classification` (via `Classification`'s próprio
        // `Serialize`) e `ConsistencyEntry`/`PairComparison` (via
        // `.label()`) precisam produzir a mesma string no JSON final —
        // "NoEdge" (nome cru da variante Rust) vs "NO_EDGE" (`.label()`)
        // já divergiram uma vez neste módulo.
        let row = row_with("X", "1h", "3m", "s", Classification::FrictionLimited);
        let json = serde_json::to_string(&row).unwrap();
        assert!(json.contains("\"classification\":\"FRICTION_LIMITED\""));
        assert_eq!(Classification::FrictionLimited.label(), "FRICTION_LIMITED");
    }

    #[test]
    fn load_all_reports_skips_its_own_diagnostic_json_output() {
        // Regressão: rodar `diagnose` de novo sobre um `results_dir` que já
        // contém um `diagnostic.json` de uma rodada anterior não pode
        // tentar desserializá-lo como se fosse um `ExperimentReport` — os
        // formatos são diferentes por completo.
        let dir = std::env::temp_dir().join(format!(
            "experiments-diagnostic-test-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("diagnostic.json"), "{ not an ExperimentReport }").unwrap();

        let report = report_with(50, Decimal::new(1, 2), Decimal::new(1, 2));
        std::fs::write(
            dir.join("real_report.json"),
            serde_json::to_string(&report).unwrap(),
        )
        .unwrap();

        let loaded = load_all_reports(&dir).unwrap();
        assert_eq!(loaded.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
