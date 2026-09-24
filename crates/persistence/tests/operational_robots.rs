//! Integração principal do dashboard operacional (Fase 3.5):
//! criar robô → habilitar candidatas → desabilitar no stop.
//!
//! Requer Postgres (`docker compose up -d`):
//!
//! ```bash
//! cargo test -p persistence --test operational_robots -- --ignored --nocapture
//! ```

use chrono::Utc;
use domain::StrategyId;
use persistence::operational_robots::{self, OperationalRobot, RobotStatus};
use persistence::strategy_configs::StrategyConfigRecord;
use rust_decimal_macros::dec;
use serde_json::json;

fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://quant:quant@localhost:55432/quant_engine".to_string())
}

#[tokio::test]
#[ignore = "requires Docker Postgres; see module docs"]
async fn create_start_stop_toggles_strategy_configs_enabled() {
    let pool = persistence::connect(&database_url())
        .await
        .expect("connect to Postgres");
    persistence::run_migrations(&pool)
        .await
        .expect("run migrations");

    let robot_id = format!("op-test-{}", Utc::now().timestamp_millis());
    let kind = "momentum";
    let instance_id = operational_robots::strategy_instance_id(&robot_id, kind);
    let now = Utc::now();

    let robot = OperationalRobot {
        id: robot_id.clone(),
        name: "Operational Test Robot".into(),
        symbol: "BTC/USDT".into(),
        timeframe: "1m".into(),
        candidate_kinds: vec![kind.into()],
        paper_capital: dec!(10000),
        status: RobotStatus::Stopped,
        created_at: now,
        updated_at: now,
    };
    operational_robots::insert(&pool, &robot)
        .await
        .expect("insert robot");

    let sid = StrategyId::new(&instance_id).unwrap();
    persistence::strategy_configs::upsert(
        &pool,
        &StrategyConfigRecord {
            id: sid.clone(),
            strategy_kind: kind.into(),
            params: json!({}),
            supported_asset_classes: vec![domain::AssetClass::Crypto],
            required_market_data: vec![domain::MarketDataKind::Ohlcv],
            enabled: false,
        },
    )
    .await
    .expect("upsert config");

    assert!(!persistence::strategy_configs::is_enabled(&pool, &sid)
        .await
        .unwrap());

    operational_robots::set_status(&pool, &robot_id, RobotStatus::Running)
        .await
        .expect("set running");
    persistence::strategy_configs::set_enabled(&pool, &sid, true)
        .await
        .expect("enable");
    assert!(persistence::strategy_configs::is_enabled(&pool, &sid)
        .await
        .unwrap());

    let running = operational_robots::list_running(&pool).await.unwrap();
    assert!(
        running.iter().any(|r| r.id == robot_id),
        "robot should appear in list_running"
    );

    operational_robots::set_status(&pool, &robot_id, RobotStatus::Stopped)
        .await
        .expect("set stopped");
    persistence::strategy_configs::set_enabled(&pool, &sid, false)
        .await
        .expect("disable");
    assert!(!persistence::strategy_configs::is_enabled(&pool, &sid)
        .await
        .unwrap());

    let disabled = persistence::strategy_configs::list_disabled_ids(&pool)
        .await
        .unwrap();
    assert!(
        disabled.iter().any(|d| d.as_str() == instance_id),
        "stopped instance must be listed as disabled for the pipeline stop-gate"
    );
}
