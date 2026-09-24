use sqlx::PgPool;

/// Estado compartilhado dos handlers: só a pool do Postgres. A UI não
/// mantém nenhum estado próprio além disso — tudo é lido do banco a cada
/// requisição, que é a fonte de verdade escrita pelo pipeline (`app`).
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
}
