//! Smoke test de ponta a ponta que comprova o funcionamento do pipeline
//! completo contra infraestrutura real:
//!
//! ```text
//! Binance data -> Strategy -> Signal -> Risk -> Paper execution -> Portfolio -> PostgreSQL
//! ```
//!
//! Duas coisas são deliberadamente reais, sem mock: a chamada REST à
//! Binance (comprovando que `market_data::BinanceMarketData` realmente
//! conversa com a exchange e que `binance::normalize` realmente faz o parse
//! da resposta) e o Postgres (comprovando que `persistence` realmente
//! escreve de verdade, via a instância local do `docker-compose.yml`). Os
//! dados que disparam a estratégia são sintéticos e acrescentados *depois*
//! das candles reais buscadas, dando continuidade à mesma série de
//! instrumento/timeframe — é isso que torna o teste determinístico (um
//! order book real e ao vivo não produz um crossover sob demanda de forma
//! confiável) sem falsear as partes que importam: conectividade com a
//! exchange, normalização, checagem de compatibilidade, avaliação de risco,
//! execução simulada, contabilidade da carteira e persistência todas rodam
//! como o código de produção real, sem modificações.
//!
//! Requer o Postgres do Docker (`docker compose up -d`) e acesso de rede à
//! API pública da Binance. Não roda por padrão com
//! `cargo test --workspace` (ver README §15) por causa dessa dependência
//! externa:
//!
//! ```bash
//! docker compose up -d
//! cargo test -p app --test smoke_test -- --ignored --nocapture
//! ```

use std::collections::{HashMap, HashSet};

use app::{pipeline, setup};
use chrono::{Duration, Utc};
use domain::{InstrumentId, MarketEvent, Money, PositionStatus, Side, StrategyId, Timeframe};
use execution::{PaperBroker, PaperBrokerConfig};
use market_data::{BinanceMarketData, MarketDataProvider};
use portfolio::PortfolioManager;
use risk::{RiskConfig, RiskEngine};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::json;
use strategies::generic::{EmaCrossoverParams, EmaCrossoverStrategy};
use strategies::{Strategy, StrategyRegistry};

fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://quant:quant@localhost:55432/quant_engine".to_string())
}

/// Monta uma série determinística de candles de queda seguida de alta, em
/// continuidade cronológica a partir de `after` e escalada sobre
/// `base_price` para permanecer realista independentemente do preço atual
/// do BTC. É exatamente esta forma relativa que os testes unitários de
/// `strategies::generic::ema_crossover` usam para produzir de maneira
/// confiável um crossover de alta com períodos fast/slow de 2/4.
fn synthetic_bullish_crossover_candles(
    instrument_id: InstrumentId,
    base_price: Decimal,
    after: chrono::DateTime<chrono::Utc>,
) -> Vec<domain::Candle> {
    let factors = [
        dec!(1.00),
        dec!(0.90),
        dec!(0.80),
        dec!(0.70),
        dec!(0.90),
        dec!(1.20),
        dec!(1.50),
    ];

    factors
        .iter()
        .enumerate()
        .map(|(i, factor)| {
            let close = base_price * factor;
            let open_time = after + Duration::minutes(i as i64 + 1);
            domain::Candle {
                instrument_id,
                timeframe: Timeframe::M1,
                open_time,
                close_time: open_time + Duration::minutes(1),
                open: close,
                high: close,
                low: close,
                close,
                volume: dec!(1),
                is_closed: true,
            }
        })
        .collect()
}

#[tokio::test]
#[ignore = "requires Docker Postgres and network access to Binance; see module docs"]
async fn binance_to_signal_to_risk_to_paper_execution_to_portfolio_to_postgres() {
    // --- Postgres real: conectar + migrar ---------------------------------
    let pool = persistence::connect(&database_url())
        .await
        .expect("connect to Postgres (did you run `docker compose up -d`?)");
    persistence::run_migrations(&pool)
        .await
        .expect("run migrations");

    // --- Binance real: buscar metadados do instrumento + candles recentes -
    let provider = BinanceMarketData::public();
    let base = domain::Asset::new("BTC").unwrap();
    let quote = domain::Asset::new("USDT").unwrap();
    let mut instrument = provider
        .fetch_instrument(&base, &quote)
        .await
        .expect("fetch BTC/USDT instrument metadata from Binance");
    assert_eq!(instrument.symbol.as_str(), "BTC/USDT");
    assert!(instrument.min_notional > Decimal::ZERO);

    // O Postgres é a autoridade sobre o `InstrumentId` (ver docs de
    // `persistence::instruments::upsert`, ADR-9) — reaproveita o id já
    // registrado para BTC/USDT antes de marcar qualquer candle com ele,
    // exatamente como `setup::load_instruments` faz.
    let first_upsert_id = persistence::instruments::upsert(&pool, &instrument)
        .await
        .expect("persist instrument");
    instrument.id = first_upsert_id;
    let second_upsert_id = persistence::instruments::upsert(&pool, &instrument)
        .await
        .expect("persist instrument again");
    assert_eq!(
        first_upsert_id, second_upsert_id,
        "upserting the same natural key twice must return the same authoritative id"
    );

    let real_candles = provider
        .fetch_recent_candles(&instrument, Timeframe::M1, 20)
        .await
        .expect("fetch recent BTC/USDT candles from Binance");
    assert!(
        !real_candles.is_empty(),
        "Binance returned no candles for BTC/USDT"
    );
    let last_real = real_candles.last().unwrap().clone();
    assert!(last_real.close > Decimal::ZERO);
    println!(
        "fetched {} real candles from Binance, last close = {}",
        real_candles.len(),
        last_real.close
    );

    // --- Estratégia + registro de compatibilidade -------------------------
    let strategy_id = StrategyId::new("smoke-test-ema-crossover").unwrap();
    let strategy = EmaCrossoverStrategy::new(
        strategy_id.clone(),
        EmaCrossoverParams {
            fast_period: 2,
            slow_period: 4,
        },
    );
    let requirements = strategy.requirements().clone();

    persistence::strategy_configs::upsert(
        &pool,
        &persistence::strategy_configs::StrategyConfigRecord {
            id: strategy_id.clone(),
            strategy_kind: "ema_crossover".to_string(),
            params: json!({ "fast_period": 2, "slow_period": 4 }),
            supported_asset_classes: requirements.supported_asset_classes.clone(),
            required_market_data: requirements.required_market_data.clone(),
            enabled: true,
        },
    )
    .await
    .expect("persist strategy config");

    let mut registry = StrategyRegistry::new();
    let available_market_data: HashSet<_> = provider
        .capabilities()
        .market_data_kinds
        .into_iter()
        .collect();
    registry
        .register(
            Box::new(strategy),
            std::slice::from_ref(&instrument),
            &available_market_data,
        )
        .expect("register strategy: instrument must be compatible");

    // --- Risk engine + paper broker + carteira ----------------------------
    let risk_engine = RiskEngine::new(RiskConfig {
        order_notional: Money::new(dec!(1000)),
        max_position_notional: Money::new(dec!(2000)),
        max_total_exposure: Money::new(dec!(10000)),
        max_open_positions: 5,
        // Desativados para que a posição aberta por este teste permaneça
        // aberta de forma determinística até as asserções finais, em vez
        // de disputar com uma saída por stop loss/take profit provocada
        // pelas oscilações sintéticas de preço.
        stop_loss_pct: None,
        take_profit_pct: None,
        max_daily_loss: Money::new(dec!(2000)),
    });
    let mut broker = PaperBroker::new(PaperBrokerConfig {
        maker_fee: dec!(0.0005),
        taker_fee: dec!(0.001),
        spread_bps: dec!(2),
        slippage_bps: dec!(3),
    });
    let mut portfolios = HashMap::new();
    portfolios.insert(
        "smoke-robot".to_string(),
        PortfolioManager::new(Money::new(dec!(100000))),
    );
    let mut tracker = setup::build_active_strategy_tracker();

    // --- Exercita o event loop real do pipeline::run ----------------------
    // Primeiro faz o warm-up com as candles reais buscadas (espelhando o
    // passo de warm-up do `main.rs`), descartando os sinais — depois
    // alimenta a cauda sintética determinística que tem o crossover
    // garantido.
    for candle in &real_candles {
        let _ = registry.dispatch(
            &instrument,
            &MarketEvent::Candle(candle.clone()),
            portfolios.get("smoke-robot").unwrap(),
        );
    }

    let synthetic_candles =
        synthetic_bullish_crossover_candles(instrument.id, last_real.close, last_real.close_time);

    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    for candle in synthetic_candles {
        event_tx.send(MarketEvent::Candle(candle)).unwrap();
    }
    drop(event_tx); // fecha o canal para o `pipeline::run` sair após drená-lo

    let (_shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

    let mut instrument_index = HashMap::new();
    instrument_index.insert(instrument.id, instrument.clone());

    let mut robots = vec![app::robot_runtime::RobotContext {
        id: "smoke-robot".to_string(),
        instrument_id: instrument.id,
        symbol: instrument.symbol.to_string(),
        timeframe: Timeframe::M1,
        strategy_ids: vec![strategy_id.clone()],
        paper_capital: dec!(100000),
    }];

    let mut market_views = app::robot_market::build_robot_market_views(&robots);

    pipeline::run(
        event_rx,
        &mut instrument_index,
        &mut robots,
        &mut registry,
        &mut tracker,
        &risk_engine,
        &mut broker,
        &mut portfolios,
        &mut market_views,
        &pool,
        None,
        shutdown_rx,
    )
    .await
    .expect("pipeline::run should complete cleanly once the event channel closes");

    // --- Verifica o estado da carteira em memória -------------------------
    {
        let portfolio = portfolios.get("smoke-robot").unwrap();
        assert_eq!(
            portfolio.open_positions().len(),
            1,
            "expected exactly one open position after the synthetic bullish crossover"
        );
        let position = &portfolio.open_positions()[0];
        assert_eq!(position.side, Side::Buy);
        assert_eq!(position.status, PositionStatus::Open);
        assert!(
            portfolio.cash().value() < dec!(100000),
            "cash must have been debited by the paper buy"
        );
    }

    // --- Verifica que as linhas chegaram ao Postgres real -----------------
    let persisted_instrument = persistence::instruments::find_by_id(&pool, instrument.id)
        .await
        .expect("query instrument")
        .expect("instrument row must exist");
    assert_eq!(persisted_instrument.id, instrument.id);

    let signals = persistence::signals::list_recent_for_strategy(&pool, &strategy_id, 10)
        .await
        .expect("query signals");
    assert!(
        !signals.is_empty(),
        "expected at least one signal row persisted for the strategy"
    );

    let orders = persistence::orders::list_for_instrument(&pool, instrument.id, 10)
        .await
        .expect("query orders");
    assert!(
        !orders.is_empty(),
        "expected at least one order row persisted for the instrument"
    );
    let order = &orders[0];
    assert_eq!(order.status, domain::OrderStatus::Filled);

    let fills = persistence::fills::list_for_order(&pool, order.id)
        .await
        .expect("query fills");
    assert!(
        !fills.is_empty(),
        "expected at least one fill row persisted for the order"
    );

    let open_positions = persistence::positions::list_open(&pool)
        .await
        .expect("query open positions");
    assert!(
        open_positions
            .iter()
            .any(|p| p.instrument_id == instrument.id && p.strategy_id == strategy_id),
        "expected the open position to be persisted"
    );

    let snapshots = persistence::robot_portfolio_snapshots::latest_for_robot(&pool, "smoke-robot")
        .await
        .expect("query robot portfolio snapshot");
    assert!(
        snapshots.is_some(),
        "expected a final robot portfolio snapshot to be persisted"
    );

    println!(
        "smoke test OK: {} signal(s), {} order(s), {} fill(s), 1 open position, robot snapshot \
         persisted to Postgres",
        signals.len(),
        orders.len(),
        fills.len(),
    );

    // Fecha a posição e persiste o estado final. O smoke test compartilha o
    // mesmo Postgres local do `quant-engine`; deixar a posição OPEN (e, em
    // execuções repetidas, *várias* OPEN no mesmo BTC/USDT para esta mesma
    // estratégia) faz `restore_robot_portfolios` rejeitar o próximo `cargo run`
    // por inconsistência — a invariante é "no máximo uma aberta por
    // (instrumento, estratégia)" (ADR-20), e este teste usa uma única
    // estratégia.
    let mark = {
        let portfolio = portfolios.get("smoke-robot").unwrap();
        portfolio.open_positions()[0].entry_price
    };
    let portfolio = portfolios.get_mut("smoke-robot").unwrap();
    let closed = portfolio
        .close_position(
            instrument.id,
            &strategy_id,
            mark,
            dec!(0),
            dec!(0),
            dec!(0),
            Utc::now(),
        )
        .expect("close smoke-test position for shared-Postgres cleanup");
    persistence::positions::upsert(&pool, &closed)
        .await
        .expect("persist closed smoke-test position");

    // O pipeline grava snapshots com o `close_time` das candles (que, no
    // trecho sintético, pode ficar *à frente* de `Utc::now()`). Um snapshot
    // de limpeza com wall-clock agora ficaria atrás do latest e
    // restore veria open_positions_count inconsistente.
    // Por isso o timestamp de limpeza é estritamente posterior ao latest.
    let cleanup_at =
        match persistence::robot_portfolio_snapshots::latest_for_robot(&pool, "smoke-robot")
            .await
            .expect("load latest robot snapshot for cleanup timestamp")
        {
            Some(latest) => latest.timestamp + Duration::seconds(1),
            None => Utc::now(),
        };
    let cleanup_snapshot = portfolio.snapshot(&HashMap::new(), cleanup_at);
    persistence::robot_portfolio_snapshots::insert(&pool, "smoke-robot", &cleanup_snapshot)
        .await
        .expect("persist post-cleanup robot portfolio snapshot");
}
