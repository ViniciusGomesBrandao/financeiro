//! Comprova a estabilidade do `InstrumentId` entre chamadas separadas de
//! `upsert` que compartilham a mesma chave natural
//! `(symbol, exchange, market_type)` — o cenário que ocorre a cada reinício
//! do processo, já que `Instrument::new` atribui um id aleatório novo a cada
//! chamada (inclusive quando o provider de market data busca de novo o mesmo
//! instrumento real depois de um reinício).
//!
//! Requer o Postgres do Docker (`docker compose up -d`):
//!
//! ```bash
//! docker compose up -d
//! cargo test -p persistence --test instrument_identity -- --ignored --nocapture
//! ```

use domain::{Asset, AssetClass, Exchange, Instrument, MarketType, Symbol};
use rust_decimal_macros::dec;

fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://quant:quant@localhost:55432/quant_engine".to_string())
}

fn synthetic_instrument() -> Instrument {
    // Um symbol com baixa chance de colidir com dados reais escritos por
    // outros testes (por exemplo o smoke test do crate app, que usa o
    // BTC/USDT real).
    let base = Asset::new("ZZZTEST").unwrap();
    let quote = Asset::new("QQQTEST").unwrap();
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

#[tokio::test]
#[ignore = "requires Docker Postgres; see module docs"]
async fn upsert_reuses_the_existing_id_across_separate_calls() {
    let pool = persistence::connect(&database_url())
        .await
        .expect("connect to Postgres (did you run `docker compose up -d`?)");
    persistence::run_migrations(&pool)
        .await
        .expect("run migrations");

    // Chamada 1: independentemente de esta chave natural já ter uma linha
    // de uma execução anterior contra este mesmo Postgres (persistente e
    // compartilhado), este upsert fixa *algum* id autoritativo.
    let first_call = synthetic_instrument();
    let id_after_first_upsert = persistence::instruments::upsert(&pool, &first_call)
        .await
        .expect("first upsert");

    // Chamada 2: simula um reinício posterior do processo buscando de novo
    // o mesmo instrumento na exchange — `Instrument::new` atribui um id
    // aleatório *diferente*, mas a chave natural (symbol/exchange/
    // market_type) é idêntica, então isto tem de resolver para o mesmo id
    // autoritativo da chamada 1, qualquer que tenha sido o id recém gerado
    // por cada uma delas.
    let second_call = synthetic_instrument();
    assert_ne!(
        second_call.id, first_call.id,
        "Instrument::new must generate a fresh id every call (sanity check on the test itself)"
    );
    let id_after_second_upsert = persistence::instruments::upsert(&pool, &second_call)
        .await
        .expect("second upsert");

    assert_eq!(
        id_after_second_upsert, id_after_first_upsert,
        "upsert must return the same authoritative id across separate calls sharing a natural \
         key, regardless of the id either caller happened to generate"
    );

    // E a linha efetivamente persistida no Postgres concorda.
    let persisted = persistence::instruments::find_by_id(&pool, id_after_first_upsert)
        .await
        .expect("query instrument")
        .expect("instrument row must exist");
    assert_eq!(persisted.id, id_after_first_upsert);
    assert_eq!(persisted.base_asset.as_str(), "ZZZTEST");
}
