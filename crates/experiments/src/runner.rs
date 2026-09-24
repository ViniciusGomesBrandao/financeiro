//! Roda um único experimento (um instrumento, um timeframe, uma
//! estratégia) através do `backtest::BacktestRunner` já corrigido — nunca
//! combina estratégias, nunca reaproveita `PortfolioManager`/`RiskEngine`/
//! `PaperBroker`/`StrategyRegistry` entre execuções. Cada chamada a
//! `run_single_experiment` constrói os quatro do zero: é isso que garante
//! isolamento total entre estratégias, mesmo quando rodam sobre o mesmo
//! instrumento/timeframe/período.

use std::collections::HashSet;
use std::path::Path;

use backtest::BacktestRunner;
use domain::{Candle, Instrument, MarketDataKind, Timeframe};
use execution::PaperBroker;
use portfolio::PortfolioManager;
use risk::RiskEngine;
use strategies::StrategyRegistry;

use crate::error::ExperimentError;
use crate::matrix::DefaultConfig;
use crate::report::{build_report, ExperimentReport};
use crate::strategy_set::StrategyFactory;

/// Lê os candles de `symbol`/`timeframe` já baixados/derivados por
/// `historical-data` sob `data_dir`. Erro explícito (não um `Vec` vazio) se
/// o arquivo ainda não existe — sinal claro de "rode `fetch-data` antes",
/// não um resultado silenciosamente vazio.
pub fn load_candles(
    data_dir: &Path,
    symbol: &str,
    timeframe: Timeframe,
    instrument: &Instrument,
) -> Result<Vec<Candle>, ExperimentError> {
    let path = historical_data::store::candle_file_path(data_dir, symbol, timeframe);
    historical_data::store::read_candles(&path, instrument.id, timeframe)?.ok_or_else(|| {
        ExperimentError::NoLocalData {
            symbol: symbol.to_string(),
            timeframe: timeframe.as_str().to_string(),
        }
    })
}

/// Roda `strategy_factory` isoladamente contra `candles` (já recortado
/// para a janela desejada — ver `crate::window`), usando `config` para
/// risco/paper trading, e monta o `ExperimentReport` resultante. Não toca
/// disco/rede — quem chama já resolveu `instrument` e `candles`.
///
/// `strict`: quando `true` (o modo que `run-experiments` sempre usa para
/// experimentos oficiais), o recorte de candles é validado *antes* de
/// rodar o backtest — qualquer gap/duplicata/fora-de-ordem faz esta
/// função retornar `Err(ExperimentError::DataQuality)` em vez de produzir
/// um relatório sobre dados de qualidade desconhecida. Quando `false`, o
/// backtest roda normalmente e o `ExperimentReport` resultante ainda
/// carrega as mesmas contagens de qualidade (`gap_count`, etc.) para
/// auditoria — útil hoje só em teste/exploração; não há como pedir isso
/// pela CLI de `run-experiments`.
///
/// Nota de design para uma futura extensão (não implementada agora): se
/// algum dia permitirmos continuar um backtest através de um gap
/// conhecido, o `features::FeatureEngine` interno ao
/// `BacktestRunner`/`FeatureStrategyAdapter` precisaria reiniciar seu
/// warm-up logo após a descontinuidade — os indicadores acumulados antes
/// do gap (EMA, janelas móveis, regressão, ...) não podem ser tratados
/// como contínuos através de candles que não existem, ou o valor do
/// indicador no primeiro candle pós-gap seria calculado como se o tempo
/// não tivesse passado.
#[allow(clippy::too_many_arguments)]
pub async fn run_single_experiment(
    instrument: &Instrument,
    symbol: &str,
    timeframe: Timeframe,
    strategy_kind: &str,
    window: &str,
    strategy_factory: StrategyFactory,
    candles: Vec<Candle>,
    config: &DefaultConfig,
    strict: bool,
) -> Result<ExperimentReport, ExperimentError> {
    if strict {
        let data_quality = historical_data::validate(&candles, timeframe);
        if !data_quality.is_clean() {
            return Err(ExperimentError::DataQuality {
                symbol: symbol.to_string(),
                timeframe: timeframe.as_str().to_string(),
                window: window.to_string(),
                gaps: data_quality.gaps.len(),
                missing_candles: data_quality.total_missing_candles(),
                duplicates: data_quality.duplicates.len(),
                out_of_order: data_quality.out_of_order.len(),
            });
        }
    }

    let mut registry = StrategyRegistry::new();
    let strategy = strategy_factory();
    let available_market_data: HashSet<MarketDataKind> = HashSet::from([MarketDataKind::Ohlcv]);
    registry.register(
        strategy,
        std::slice::from_ref(instrument),
        &available_market_data,
    )?;

    let risk_engine = RiskEngine::new(config.risk.clone());
    let mut broker = PaperBroker::new(config.broker);
    let mut portfolio = PortfolioManager::new(config.initial_balance);
    let backtest_runner = BacktestRunner::new(vec![instrument.clone()]);

    let backtest_report = backtest_runner
        .run(
            candles.clone(),
            &mut registry,
            &risk_engine,
            &mut broker,
            &mut portfolio,
        )
        .await?;

    Ok(build_report(
        symbol,
        timeframe,
        strategy_kind,
        window,
        &candles,
        config.initial_balance.value(),
        &backtest_report,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use domain::{Asset, AssetClass, Exchange, InstrumentId, MarketType, Symbol};
    use rust_decimal_macros::dec;

    fn instrument() -> Instrument {
        let base = Asset::new("BTC").unwrap();
        let quote = Asset::new("USDT").unwrap();
        Instrument::new(
            Symbol::from_pair(&base, &quote),
            base,
            quote,
            AssetClass::Crypto,
            Exchange::Binance,
            MarketType::Spot,
            dec!(0.01),
            dec!(0.0001),
            dec!(0.0001),
            dec!(10),
        )
    }

    fn candle(instrument_id: InstrumentId, close: rust_decimal::Decimal, minute: i64) -> Candle {
        let open_time =
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap() + chrono::Duration::minutes(minute);
        Candle {
            instrument_id,
            timeframe: Timeframe::M1,
            open_time,
            close_time: open_time + chrono::Duration::minutes(1),
            open: close,
            high: close,
            low: close,
            close,
            volume: dec!(10),
            is_closed: true,
        }
    }

    #[tokio::test]
    async fn runs_a_strategy_end_to_end_and_produces_a_populated_report() {
        let instrument = instrument();
        // Queda seguida de alta -> cruzamento de alta garantido, mesmo
        // formato já usado nos testes de `backtest`/`strategies`.
        let prices = [
            dec!(100),
            dec!(90),
            dec!(80),
            dec!(70),
            dec!(90),
            dec!(120),
            dec!(150),
        ];
        let candles: Vec<Candle> = prices
            .iter()
            .enumerate()
            .map(|(i, p)| candle(instrument.id, *p, i as i64))
            .collect();
        let config = crate::matrix::default_config();
        let factories = crate::strategy_set::all_factories();
        let (kind, factory) = factories
            .iter()
            .find(|(k, _)| *k == "ema_crossover")
            .unwrap();

        let report = run_single_experiment(
            &instrument,
            "BTCUSDT",
            Timeframe::M1,
            kind,
            "full",
            *factory,
            candles,
            &config,
            true,
        )
        .await
        .unwrap();

        assert_eq!(report.symbol, "BTCUSDT");
        assert_eq!(report.strategy_id, "ema_crossover");
        assert_eq!(report.window, "full");
        assert_eq!(report.candles_used, 7);
        assert!(report.period_start.is_some());
        assert!(report.benchmark_return_pct.is_some());
        // Série de teste contígua (1 candle por minuto, sem buracos) ->
        // qualidade de dados limpa, e isso precisa vir refletido no
        // relatório, não silenciosamente ausente.
        assert_eq!(report.gap_count, 0);
        assert_eq!(report.missing_candles_total, 0);
        // Auditoria manual precisa dos trades individuais, não só dos
        // totais agregados — o relatório precisa expor exatamente os
        // mesmos trades que os totais contam, nunca uma lista divergente.
        assert_eq!(report.closed_positions.len(), report.total_trades);
        for position in &report.closed_positions {
            assert!(position.exit_price.is_some());
            assert!(position.closed_at.is_some());
        }
    }

    #[tokio::test]
    async fn two_strategies_on_the_same_candles_never_share_state() {
        // Mesma série de candles, duas estratégias diferentes: os
        // relatórios não podem influenciar um ao outro — cada
        // `run_single_experiment` constrói Portfolio/Risk/Broker/Registry
        // do zero.
        let instrument = instrument();
        let prices = [
            dec!(100),
            dec!(90),
            dec!(80),
            dec!(70),
            dec!(90),
            dec!(120),
            dec!(150),
        ];
        let candles: Vec<Candle> = prices
            .iter()
            .enumerate()
            .map(|(i, p)| candle(instrument.id, *p, i as i64))
            .collect();
        let config = crate::matrix::default_config();
        let factories = crate::strategy_set::all_factories();

        let mut reports = Vec::new();
        for (kind, factory) in &factories[..2] {
            let report = run_single_experiment(
                &instrument,
                "BTCUSDT",
                Timeframe::M1,
                kind,
                "full",
                *factory,
                candles.clone(),
                &config,
                true,
            )
            .await
            .unwrap();
            reports.push(report);
        }

        // Cada relatório reflete só a própria estratégia isolada — os
        // saldos iniciais usados são idênticos (mesma config), então
        // qualquer contaminação de estado apareceria como resultados
        // artificialmente idênticos entre estratégias com lógicas
        // distintas, ou um panic de estado inconsistente.
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].strategy_id, factories[0].0);
        assert_eq!(reports[1].strategy_id, factories[1].0);
    }

    fn candles_with_a_gap(instrument_id: InstrumentId) -> Vec<Candle> {
        let mut candles = vec![
            candle(instrument_id, dec!(100), 0),
            candle(instrument_id, dec!(101), 1),
            // minutos 2, 3, 4 faltando de propósito
            candle(instrument_id, dec!(102), 5),
        ];
        candles.sort_by_key(|c| c.open_time);
        candles
    }

    #[tokio::test]
    async fn non_strict_mode_still_counts_and_surfaces_gaps_in_the_report() {
        // Requisito: gaps no histórico precisam ser contabilizados e
        // reportados, nunca silenciosamente ignorados — mesmo no modo não
        // estrito, em que o backtest ainda roda sobre os candles que
        // existem (o modo estrito, testado abaixo, é o usado por
        // experimentos oficiais).
        let instrument = instrument();
        let candles = candles_with_a_gap(instrument.id);
        let config = crate::matrix::default_config();
        let factories = crate::strategy_set::all_factories();
        let (kind, factory) = factories.first().unwrap();

        let report = run_single_experiment(
            &instrument,
            "BTCUSDT",
            Timeframe::M1,
            kind,
            "full",
            *factory,
            candles,
            &config,
            false,
        )
        .await
        .unwrap();

        assert_eq!(report.gap_count, 1);
        assert_eq!(report.missing_candles_total, 3);
    }

    #[tokio::test]
    async fn strict_mode_refuses_to_run_a_window_with_gaps() {
        // Experimentos oficiais (`strict: true`, o que `run-experiments`
        // sempre usa) nunca produzem um relatório sobre um recorte de
        // candles com gap — a chamada inteira falha antes do backtest
        // rodar.
        let instrument = instrument();
        let candles = candles_with_a_gap(instrument.id);
        let config = crate::matrix::default_config();
        let factories = crate::strategy_set::all_factories();
        let (kind, factory) = factories.first().unwrap();

        let result = run_single_experiment(
            &instrument,
            "BTCUSDT",
            Timeframe::M1,
            kind,
            "full",
            *factory,
            candles,
            &config,
            true,
        )
        .await;

        assert!(
            matches!(
                result,
                Err(ExperimentError::DataQuality {
                    gaps: 1,
                    missing_candles: 3,
                    ..
                })
            ),
            "expected a DataQuality error describing the single 3-candle gap, got {result:?}"
        );
    }
}
