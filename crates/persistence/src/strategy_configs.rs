use domain::{AssetClass, MarketDataKind, StrategyId};
use serde_json::Value;
use sqlx::PgPool;

use crate::codec::{
    asset_class_from_str, asset_class_to_str, market_data_kind_from_str, market_data_kind_to_str,
};
use crate::error::PersistenceError;

/// Uma configuração de estratégia persistida: para qual implementação
/// concreta (`strategy_kind`) um `StrategyId` aponta, seus parâmetros e seus
/// `StrategyRequirements` declarados (desnormalizados aqui para fins de
/// auditoria — a fonte da verdade para a validação continua sendo o
/// `strategies::StrategyRegistry` em execução, não esta tabela).
#[derive(Debug, Clone)]
pub struct StrategyConfigRecord {
    pub id: StrategyId,
    pub strategy_kind: String,
    pub params: Value,
    pub supported_asset_classes: Vec<AssetClass>,
    pub required_market_data: Vec<MarketDataKind>,
    pub enabled: bool,
}

#[derive(sqlx::FromRow)]
struct StrategyConfigRow {
    id: String,
    strategy_kind: String,
    params: Value,
    supported_asset_classes: Vec<String>,
    required_market_data: Vec<String>,
    enabled: bool,
}

fn row_to_record(row: StrategyConfigRow) -> Result<StrategyConfigRecord, PersistenceError> {
    Ok(StrategyConfigRecord {
        id: StrategyId::new(row.id)
            .map_err(|e| PersistenceError::Database(sqlx::Error::Decode(e.to_string().into())))?,
        strategy_kind: row.strategy_kind,
        params: row.params,
        supported_asset_classes: row
            .supported_asset_classes
            .iter()
            .map(|s| asset_class_from_str(s))
            .collect::<Result<_, _>>()?,
        required_market_data: row
            .required_market_data
            .iter()
            .map(|s| market_data_kind_from_str(s))
            .collect::<Result<_, _>>()?,
        enabled: row.enabled,
    })
}

pub async fn upsert(pool: &PgPool, record: &StrategyConfigRecord) -> Result<(), PersistenceError> {
    let supported: Vec<&str> = record
        .supported_asset_classes
        .iter()
        .map(|c| asset_class_to_str(*c))
        .collect();
    let required: Vec<&str> = record
        .required_market_data
        .iter()
        .map(|k| market_data_kind_to_str(*k))
        .collect();

    sqlx::query(
        r#"
        INSERT INTO strategy_configs (id, strategy_kind, params, supported_asset_classes, required_market_data, enabled)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (id) DO UPDATE SET
            strategy_kind = EXCLUDED.strategy_kind,
            params = EXCLUDED.params,
            supported_asset_classes = EXCLUDED.supported_asset_classes,
            required_market_data = EXCLUDED.required_market_data,
            enabled = EXCLUDED.enabled
        "#,
    )
    .bind(record.id.as_str())
    .bind(&record.strategy_kind)
    .bind(&record.params)
    .bind(&supported)
    .bind(&required)
    .bind(record.enabled)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_enabled(
    pool: &PgPool,
    id: &StrategyId,
    enabled: bool,
) -> Result<bool, PersistenceError> {
    let result = sqlx::query(r#"UPDATE strategy_configs SET enabled = $2 WHERE id = $1"#)
        .bind(id.as_str())
        .bind(enabled)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn is_enabled(pool: &PgPool, id: &StrategyId) -> Result<bool, PersistenceError> {
    let row: Option<(bool,)> =
        sqlx::query_as(r#"SELECT enabled FROM strategy_configs WHERE id = $1"#)
            .bind(id.as_str())
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|r| r.0).unwrap_or(false))
}

/// IDs com `enabled = false`. Usado pelo pipeline para bloquear sinais Long
/// de instâncias desligadas pelo dashboard sem precisar reiniciar o processo
/// (stop efetivo). Instâncias nunca vistas na tabela não entram aqui — o
/// registry só as carrega se `enabled` era true no boot.
pub async fn list_disabled_ids(pool: &PgPool) -> Result<Vec<StrategyId>, PersistenceError> {
    let rows: Vec<(String,)> =
        sqlx::query_as(r#"SELECT id FROM strategy_configs WHERE enabled = false"#)
            .fetch_all(pool)
            .await?;
    rows.into_iter()
        .map(|(id,)| {
            StrategyId::new(id)
                .map_err(|e| PersistenceError::Database(sqlx::Error::Decode(e.to_string().into())))
        })
        .collect()
}

pub async fn list_enabled(pool: &PgPool) -> Result<Vec<StrategyConfigRecord>, PersistenceError> {
    let rows: Vec<StrategyConfigRow> = sqlx::query_as(
        r#"SELECT id, strategy_kind, params, supported_asset_classes, required_market_data, enabled
           FROM strategy_configs WHERE enabled = true ORDER BY id"#,
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter().map(row_to_record).collect()
}
