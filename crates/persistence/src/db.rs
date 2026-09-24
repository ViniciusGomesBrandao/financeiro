use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tracing::info;

use crate::error::PersistenceError;

pub async fn connect(database_url: &str) -> Result<PgPool, PersistenceError> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;
    Ok(pool)
}

/// Aplica toda migration em `<raiz do repo>/migrations` que ainda não foi
/// executada contra `pool`. Seguro chamar a cada inicialização — migrations
/// já aplicadas são ignoradas (incluindo a 17, que relaxa FKs de robot_id
/// na telemetria do Judge para o modo legado).
pub async fn run_migrations(pool: &PgPool) -> Result<(), PersistenceError> {
    info!("running database migrations");
    sqlx::migrate!("../../migrations").run(pool).await?;
    info!("database migrations up to date");
    Ok(())
}
