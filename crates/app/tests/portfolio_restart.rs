//! Teste de integração: prova que o estado do `PortfolioManager` sobrevive
//! a um restart do `quant-engine`.
//!
//! Simula:
//!   1. "processo 1" abre uma posição diretamente via `PortfolioManager`
//!      (espelhando o que `execution::PaperBroker` faz em produção) e
//!      persiste a posição + um snapshot de portfólio, exatamente como
//!      `app::pipeline` já faz a cada candle fechada.
//!   2. "restart" — chama a mesma função que `main.rs` usa no bootstrap
//!      (`app::setup::restore_portfolio`) para reconstruir o
//!      `PortfolioManager` a partir do que está persistido.
//!   3. Confirma que a posição restaurada é a mesma (mesmo id, quantidade,
//!      preço de entrada, fees/spread/slippage) — não uma recriada.
//!   4. Alimenta um novo sinal Long para o mesmo instrumento através do
//!      `risk::RiskEngine::evaluate` real e confirma que é rejeitado por
//!      `PositionAlreadyOpen` — provando que o motor de risco realmente
//!      enxerga o estado restaurado, não só que `PortfolioManager` o
//!      reporta.
//!
//! Requer Docker Postgres (`docker compose up -d`):
//!
//! ```bash
//! docker compose up -d
//! cargo test -p app --test portfolio_restart -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `--test-threads=1` é necessário aqui, não opcional: as três funções de
//! teste deste arquivo escrevem em `portfolio_snapshots`/`positions`, que
//! são tabelas globais compartilhadas por todos os testes deste binário
//! (não há isolamento por schema/transação). Rodando em paralelo (o padrão
//! do `cargo test`), uma pode ler um estado transitório escrito por outra
//! no meio da execução e falhar por uma inconsistência que não é real —
//! confirmado ao reproduzir a falha e verificar que cada teste passa
//! isoladamente. Isso é uma característica da estratégia de teste (banco
//! real e persistente, não um mock), não um bug de produção.

use std::collections::HashMap;

use app::setup::restore_portfolio;
use chrono::Utc;
use domain::{
    Asset, AssetClass, Exchange, Instrument, MarketType, Money, Price, Side, Signal,
    SignalDirection, StrategyId, Symbol,
};
use persistence::strategy_configs::StrategyConfigRecord;
use portfolio::PortfolioManager;
use risk::{RejectionReason, RiskConfig, RiskDecision, RiskEngine};
use rust_decimal_macros::dec;

fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://quant:quant@localhost:55432/quant_engine".to_string())
}

fn synthetic_instrument() -> Instrument {
    let base = Asset::new("RESTARTTEST").unwrap();
    let quote = Asset::new("USDT").unwrap();
    Instrument::new(
        Symbol::from_pair(&base, &quote),
        base,
        quote,
        AssetClass::Crypto,
        Exchange::Other("test-exchange".to_string()),
        MarketType::Spot,
        dec!(0.01),
        dec!(0.0001),
        dec!(0.0001),
        dec!(10),
    )
}

fn base_risk_config() -> RiskConfig {
    RiskConfig {
        order_notional: Money::new(dec!(1000)),
        max_position_notional: Money::new(dec!(2000)),
        max_total_exposure: Money::new(dec!(10000)),
        max_open_positions: 5,
        stop_loss_pct: None,
        take_profit_pct: None,
        max_daily_loss: Money::new(dec!(2000)),
    }
}

#[tokio::test]
#[ignore = "requires Docker Postgres; see module docs"]
async fn open_position_survives_restart_and_blocks_new_entry() {
    let pool = persistence::connect(&database_url())
        .await
        .expect("connect to Postgres (did you run `docker compose up -d`?)");
    persistence::run_migrations(&pool)
        .await
        .expect("run migrations");

    // Natural key dedicada a este teste — upsert garante um instrument_id
    // estável mesmo que o teste já tenha rodado antes contra este mesmo
    // Postgres persistente (ver ADR-9).
    let mut instrument = synthetic_instrument();
    instrument.id = persistence::instruments::upsert(&pool, &instrument)
        .await
        .expect("upsert instrument");

    let strategy_id = StrategyId::new("portfolio-restart-check").unwrap();
    persistence::strategy_configs::upsert(
        &pool,
        &StrategyConfigRecord {
            id: strategy_id.clone(),
            strategy_kind: "test".to_string(),
            params: serde_json::json!({}),
            supported_asset_classes: vec![AssetClass::Crypto],
            required_market_data: vec![],
            enabled: true,
        },
    )
    .await
    .expect("upsert strategy config");

    // --- "processo 1": abre uma posição e persiste, como o pipeline faria ---
    let initial_cash = Money::new(dec!(100000));
    let mut portfolio_before = PortfolioManager::new(initial_cash);
    let opened = portfolio_before.open_position(
        instrument.id,
        strategy_id.clone(),
        Side::Buy,
        dec!(1),
        dec!(50000),
        dec!(50),
        dec!(10),
        dec!(15),
        Utc::now(),
    );
    persistence::positions::upsert(&pool, &opened)
        .await
        .expect("persist opened position");

    // `portfolio_snapshots` é uma tabela global (um portfólio só, por
    // design de produção — ver README), compartilhada com qualquer outro
    // teste/execução contra este mesmo Postgres. `restore_portfolio` usa o
    // snapshot de maior `timestamp`; o smoke test, por exemplo, grava
    // snapshots com o `close_time` de suas candles sintéticas, que pode
    // ficar à frente do horário real. Para que este teste dependa só do
    // que ele mesmo gravou (não de quem rodou por último em outro
    // processo), o snapshot é gravado com um timestamp deliberadamente
    // muito à frente — não representa "agora", só garante que este é o
    // snapshot mais recente no momento do restore logo abaixo.
    let snapshot_timestamp = Utc::now() + chrono::Duration::days(3650);
    let mut snapshot_before = portfolio_before.snapshot(&HashMap::new(), snapshot_timestamp);
    // `portfolio_before` é um `PortfolioManager` novo, criado só para este
    // teste — seu `open_positions_count` (sempre 1: a posição que acabamos
    // de abrir acima) não reflete o total real na tabela `positions`
    // compartilhada, que pode ter outras posições legitimamente abertas
    // por uma execução real do `quant-engine` contra este mesmo Postgres
    // de desenvolvimento. `restore_portfolio` valida esse total contra o
    // que `list_open` retorna de verdade, então o snapshot gravado aqui
    // precisa reportar a contagem real, não a visão isolada deste
    // portfólio de teste.
    let real_open_count = persistence::positions::list_open(&pool)
        .await
        .expect("count real open positions before snapshotting")
        .len() as u32;
    snapshot_before.open_positions_count = real_open_count;
    persistence::portfolio_snapshots::insert(&pool, &snapshot_before)
        .await
        .expect("persist portfolio snapshot");

    // --- "restart": reconstrói via a mesma função que main.rs usa ---
    let restored = restore_portfolio(&pool, initial_cash)
        .await
        .expect("restore_portfolio should succeed with consistent persisted state");

    // A mesma posição volta — não uma recriada/duplicada. Não presume que
    // o portfólio inteiro tenha só esta posição: o Postgres é
    // compartilhado com outros testes/execuções (ex.: o smoke test, que
    // pode legitimamente ter sua própria posição aberta em outro
    // instrumento), então a verificação é escopada ao instrumento deste
    // teste, não a `open_positions().len()` do portfólio inteiro.
    let restored_position = restored
        .open_position_for(instrument.id, &strategy_id)
        .expect("the position opened before the restart must come back after it");
    assert_eq!(restored_position.id, opened.id);
    assert_eq!(restored_position.instrument_id, opened.instrument_id);
    assert_eq!(restored_position.quantity, opened.quantity);
    assert_eq!(restored_position.entry_price, opened.entry_price);
    assert_eq!(restored_position.fees_paid, opened.fees_paid);
    assert_eq!(restored_position.spread_paid, opened.spread_paid);
    assert_eq!(restored_position.slippage_paid, opened.slippage_paid);
    // O caixa restaurado é o persistido no snapshot, não o initial_cash.
    assert_eq!(restored.cash(), portfolio_before.cash());

    // --- novo sinal Long para o mesmo instrumento deve ser bloqueado ---
    let risk_engine = RiskEngine::new(base_risk_config());
    let signal = Signal::new(
        strategy_id.clone(),
        instrument.id,
        SignalDirection::Long,
        0.9,
        Utc::now(),
        None,
        None,
        serde_json::Value::Null,
    )
    .unwrap();
    let price = Price::new(dec!(51000)).unwrap();
    let risk_snapshot = restored.risk_snapshot(Utc::now());
    let decision = risk_engine.evaluate(&signal, &instrument, price, &risk_snapshot);

    assert_eq!(
        decision,
        RiskDecision::Rejected(RejectionReason::PositionAlreadyOpen),
        "risk engine must see the restored position and refuse a second entry on the same \
         instrument, exactly as it would within a single uninterrupted run"
    );

    // Fecha a posição e persiste o fechamento, para que o teste não deixe
    // uma posição aberta "orfã" para trás a cada execução — rodar este
    // teste várias vezes contra o mesmo Postgres persistente sem isso
    // acabaria violando a própria invariante que `restore_portfolio`
    // valida (no máximo uma posição aberta por instrumento), pelo motivo
    // errado (resíduo de teste, não uma inconsistência real).
    let mut restored = restored;
    let closed = restored
        .close_position(
            instrument.id,
            &strategy_id,
            dec!(51000),
            dec!(5),
            dec!(2),
            dec!(3),
            Utc::now(),
        )
        .expect("closing the restored position should succeed");
    persistence::positions::upsert(&pool, &closed)
        .await
        .expect("persist closed position");

    // Este `strategy_id` é deliberadamente diferente do padrão de exclusão
    // `smoke-test-`/`restart-test-` (ver `app::setup::is_test_strategy`) —
    // este teste exercita o caminho NORMAL de `restore_portfolio` (a
    // posição precisa voltar de verdade, não ser filtrada). Isso significa
    // que, ao contrário de uma posição de teste "de propósito" excluída,
    // esta aqui SOMA no `realized_pnl_total` real que uma instância ao vivo
    // do `quant-engine` calcularia ao reiniciar contra este mesmo Postgres
    // — sem apagar a linha, cada execução deste teste acrescentaria +945
    // (o P&L sintético do cenário acima) ao P&L "real" do dashboard, a
    // exata classe de contaminação que este arquivo existe para testar que
    // NÃO acontece. Por isso a linha é removida aqui, e não só fechada.
    sqlx::query("DELETE FROM positions WHERE id = $1")
        .bind(closed.id)
        .execute(&pool)
        .await
        .expect("delete this test's scratch position");

    // Remove os snapshots com timestamp futuro que este teste inseriu só
    // para isolar o restore acima. Deixá-los no banco compartilhado faria
    // `portfolio_snapshots::latest` preferi-los para sempre sobre qualquer
    // snapshot real do `quant-engine`/smoke test (que usam `Utc::now()`),
    // contaminando o caixa restaurado no próximo `cargo run`.
    sqlx::query(r#"DELETE FROM portfolio_snapshots WHERE "timestamp" >= $1"#)
        .bind(snapshot_timestamp)
        .execute(&pool)
        .await
        .expect("delete this test's future-dated portfolio snapshots");
}

/// Nota sobre este teste: como ele roda contra um Postgres persistente e
/// compartilhado (o mesmo usado por outros testes de integração e pelo
/// smoke test), não é possível garantir de forma confiável que nenhum
/// `portfolio_snapshots`/posição já exista globalmente — então esta
/// verificação não assume um banco vazio nem compara `cash()` contra um
/// valor absoluto. O que é verdade independentemente do estado global é
/// que `restore_portfolio` nunca "inventa" uma posição aberta para um
/// instrumento que nunca teve nenhuma persistida.
#[tokio::test]
#[ignore = "requires Docker Postgres; see module docs"]
async fn restore_never_fabricates_a_position_for_an_untouched_instrument() {
    let pool = persistence::connect(&database_url())
        .await
        .expect("connect to Postgres (did you run `docker compose up -d`?)");
    persistence::run_migrations(&pool)
        .await
        .expect("run migrations");

    // Natural key própria deste teste — nunca teve nenhuma posição aberta.
    let base = Asset::new("FRESHSTART").unwrap();
    let quote = Asset::new("USDT").unwrap();
    let instrument = Instrument::new(
        Symbol::from_pair(&base, &quote),
        base,
        quote,
        AssetClass::Crypto,
        Exchange::Other("test-exchange".to_string()),
        MarketType::Spot,
        dec!(0.01),
        dec!(0.0001),
        dec!(0.0001),
        dec!(10),
    );
    let instrument_id = persistence::instruments::upsert(&pool, &instrument)
        .await
        .expect("upsert instrument");

    let restored = restore_portfolio(&pool, Money::new(dec!(100000)))
        .await
        .expect("restore_portfolio should succeed regardless of unrelated global state");

    assert!(
        restored
            .open_positions()
            .iter()
            .all(|p| p.instrument_id != instrument_id),
        "an instrument with no persisted position must not appear as open after restore"
    );
}

/// Regressão para a contaminação encontrada em auditoria: `strategy_id`s de
/// teste (`smoke-test-*`/`restart-test-*`) persistidos no mesmo Postgres
/// compartilhado não podem entrar no ledger do `PortfolioManager` que
/// `restore_portfolio` reconstrói para a instância ao vivo — nem no P&L
/// realizado (via `closed_positions`), nem como posição aberta (o que
/// poderia bloquear uma estratégia real de operar o mesmo instrumento,
/// já que uma posição aberta é rastreada por instrumento, não por
/// estratégia). Reproduz exatamente o cenário real que causou a
/// divergência: uma posição de teste com P&L sintético muito maior que
/// qualquer trade real (na prática, foi `crates/app/tests/portfolio_restart.rs`
/// usando um `strategy_id` que colidia com o próprio padrão de exclusão —
/// por isso este teste agora usa `portfolio-restart-check`, fora do padrão,
/// e testa a exclusão separadamente com um id que bate no padrão de
/// propósito).
#[tokio::test]
#[ignore = "requires Docker Postgres; see module docs"]
async fn restore_portfolio_excludes_test_strategy_positions_from_the_live_ledger() {
    let pool = persistence::connect(&database_url())
        .await
        .expect("connect to Postgres (did you run `docker compose up -d`?)");
    persistence::run_migrations(&pool)
        .await
        .expect("run migrations");

    let base = Asset::new("CONTAMTEST").unwrap();
    let quote = Asset::new("USDT").unwrap();
    let instrument = Instrument::new(
        Symbol::from_pair(&base, &quote),
        base,
        quote,
        AssetClass::Crypto,
        Exchange::Other("test-exchange".to_string()),
        MarketType::Spot,
        dec!(0.01),
        dec!(0.0001),
        dec!(0.0001),
        dec!(10),
    );
    let instrument_id = persistence::instruments::upsert(&pool, &instrument)
        .await
        .expect("upsert instrument");

    // strategy_id bate de propósito no padrão de exclusão
    // (`is_test_strategy` em `app::setup` e `web::handlers`) — é
    // exatamente essa exclusão que este teste verifica.
    let test_strategy_id = StrategyId::new("restart-test-contamination-check").unwrap();
    persistence::strategy_configs::upsert(
        &pool,
        &StrategyConfigRecord {
            id: test_strategy_id.clone(),
            strategy_kind: "test".to_string(),
            params: serde_json::json!({}),
            supported_asset_classes: vec![AssetClass::Crypto],
            required_market_data: vec![],
            enabled: true,
        },
    )
    .await
    .expect("upsert strategy config");

    // Um ciclo abrir+fechar com P&L sintético grande (compra a 50000,
    // venda a 51000 — a mesma ordem de grandeza que causou a divergência
    // real observada no dashboard: milhares de unidades monetárias,
    // muito acima de qualquer trade real do paper trading).
    let mut scratch_portfolio = PortfolioManager::new(Money::new(dec!(100000)));
    let opened = scratch_portfolio.open_position(
        instrument_id,
        test_strategy_id.clone(),
        Side::Buy,
        dec!(1),
        dec!(50000),
        dec!(0),
        dec!(0),
        dec!(0),
        Utc::now(),
    );
    persistence::positions::upsert(&pool, &opened)
        .await
        .expect("persist opened test-strategy position");
    let closed = scratch_portfolio
        .close_position(
            instrument_id,
            &test_strategy_id,
            dec!(51000),
            dec!(0),
            dec!(0),
            dec!(0),
            Utc::now(),
        )
        .expect("closing the scratch position should succeed");
    persistence::positions::upsert(&pool, &closed)
        .await
        .expect("persist closed test-strategy position");

    let realized_pnl_before = scratch_portfolio.realized_pnl_total();
    assert_eq!(
        realized_pnl_before,
        Money::new(dec!(1000)),
        "sanity check: o ciclo sintético realmente produz um P&L bem maior que qualquer trade \
         real, para que a asserção abaixo prove algo"
    );

    let restored = restore_portfolio(&pool, Money::new(dec!(100000)))
        .await
        .expect("restore_portfolio should succeed and simply exclude the test-strategy position");

    assert!(
        restored
            .closed_positions()
            .iter()
            .all(|p| p.strategy_id != test_strategy_id),
        "a closed position from a test-prefixed strategy_id must never reach the live \
         PortfolioManager's closed_positions"
    );
    assert!(
        restored
            .open_positions()
            .iter()
            .all(|p| p.instrument_id != instrument_id),
        "the test-strategy position must not occupy the live portfolio's open-position slot \
         for this instrument, which would otherwise block a real strategy from trading it"
    );

    // Limpa as linhas que este teste criou — não são um resíduo de
    // execução normal do sistema (como as que `smoke_test.rs`/o próprio
    // `quant-engine` deixam), são dados fabricados só para esta asserção,
    // e o padrão de exclusão as tornaria invisíveis de qualquer forma; a
    // remoção evita acumular lixo no Postgres compartilhado a cada rodada.
    sqlx::query("DELETE FROM positions WHERE strategy_id = $1")
        .bind(test_strategy_id.as_str())
        .execute(&pool)
        .await
        .expect("delete this test's scratch positions");
    sqlx::query("DELETE FROM strategy_configs WHERE id = $1")
        .bind(test_strategy_id.as_str())
        .execute(&pool)
        .await
        .expect("delete this test's scratch strategy config");
}
