use anyhow::{Context, Result};
use axum::routing::{get, patch};
use axum::Router;
use tower_http::services::{ServeDir, ServeFile};
use tracing_subscriber::EnvFilter;
use web::handlers;
use web::operational;
use web::state::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let database_url =
        std::env::var("DATABASE_URL").context("DATABASE_URL environment variable is required")?;
    // Default deliberadamente incomum (não a 8080, usada por tantos outros
    // dev servers) para não colidir com outro processo local.
    let port: u16 = std::env::var("WEB_PORT")
        .unwrap_or_else(|_| "58080".to_string())
        .parse()
        .context("WEB_PORT must be a valid port number")?;

    let pool = persistence::connect(&database_url)
        .await
        .context("connecting to Postgres")?;
    persistence::run_migrations(&pool)
        .await
        .context("running database migrations")?;

    let state = AppState { pool };

    // Frontend React buildado em `crates/web/static` (`npm run build` em
    // `crates/web/frontend`). As rotas `/api/*` continuam sendo o contrato
    // estável; o SPA é só apresentação.
    let static_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/static");
    let index = ServeFile::new(format!("{static_dir}/index.html"));
    let static_files = ServeDir::new(static_dir).not_found_service(index);

    let app = Router::new()
        .route("/api/overview", get(handlers::overview))
        .route("/api/prices", get(handlers::prices))
        .route("/api/positions", get(handlers::positions))
        .route("/api/trades", get(handlers::trades))
        .route("/api/performance", get(handlers::performance))
        .route("/api/timeline", get(handlers::timeline))
        .route("/api/strategy-catalog", get(operational::strategy_catalog))
        .route(
            "/api/robots",
            get(operational::list_robots).post(operational::create_robot),
        )
        .route("/api/robots/{id}", get(operational::get_robot_detail))
        .route(
            "/api/robots/{id}/status",
            patch(operational::set_robot_status),
        )
        .route(
            "/api/judge/evaluations",
            get(operational::judge_evaluations),
        )
        .route(
            "/api/strategy-switches",
            get(operational::strategy_switches),
        )
        .with_state(state)
        .fallback_service(static_files);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .with_context(|| format!("binding to 127.0.0.1:{port}"))?;
    tracing::info!(
        url = format!("http://127.0.0.1:{port}"),
        static_dir,
        "quant-engine-web listening (observability + operational dashboard)"
    );

    axum::serve(listener, app).await.context("serving HTTP")?;
    Ok(())
}
