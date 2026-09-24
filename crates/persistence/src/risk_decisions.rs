use chrono::{DateTime, Utc};
use domain::{InstrumentId, SignalId, StrategyId};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::PersistenceError;

/// Registro de uma decisão do Risk Engine — aprovada ou rejeitada — para
/// um sinal de estratégia ou para uma saída forçada (stop loss/take
/// profit). É o único lugar onde o motivo de uma rejeição fica gravado:
/// sinais rejeitados nunca geram uma order, então não deixam nenhum
/// rastro nas tabelas `orders`/`fills`/`positions`.
#[derive(Debug, Clone)]
pub struct RiskDecisionRecord {
    pub id: Uuid,
    /// `None` para saídas forçadas (stop loss/take profit), que não vêm
    /// de um sinal de estratégia.
    pub signal_id: Option<SignalId>,
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    /// `"signal"`, `"stop_loss"` ou `"take_profit"`.
    pub trigger: String,
    pub approved: bool,
    /// Motivo legível — preenchido para rejeições (via `Display` de
    /// `risk::RejectionReason`) e para saídas forçadas; `None` só para
    /// sinais aprovados sem nada além do óbvio a dizer.
    pub reason: Option<String>,
    /// A order resultante, se a decisão foi aprovada.
    pub order_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

pub async fn insert(pool: &PgPool, record: &RiskDecisionRecord) -> Result<(), PersistenceError> {
    sqlx::query(
        r#"
        INSERT INTO risk_decisions
            (id, signal_id, instrument_id, strategy_id, trigger, approved, reason, order_id, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(record.id)
    .bind(record.signal_id.map(|s| s.0))
    .bind(record.instrument_id.0)
    .bind(record.strategy_id.as_str())
    .bind(&record.trigger)
    .bind(record.approved)
    .bind(&record.reason)
    .bind(record.order_id)
    .bind(record.created_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Uma linha da timeline "sinal -> motivo -> decisão -> execução ->
/// fechamento/P&L", já com tudo junto via join — é o formato que a UI de
/// observabilidade serializa quase diretamente para JSON. Puramente uma
/// projeção de leitura: os campos são texto/decimal crus (não tipos de
/// `domain`) porque não há necessidade de reconstruir um objeto de domínio
/// só para exibir.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TimelineEntry {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub symbol: String,
    pub strategy_id: String,
    pub trigger: String,
    pub approved: bool,
    pub reason: Option<String>,
    pub signal_direction: Option<String>,
    pub signal_confidence: Option<f64>,
    pub order_side: Option<String>,
    pub order_quantity: Option<Decimal>,
    pub order_status: Option<String>,
    pub fill_price: Option<Decimal>,
    pub fill_fee: Option<Decimal>,
    pub fill_spread_cost: Option<Decimal>,
    pub fill_slippage_cost: Option<Decimal>,
    pub pnl_gross: Option<Decimal>,
    pub pnl_net: Option<Decimal>,
}

pub async fn list_timeline(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<TimelineEntry>, PersistenceError> {
    let rows = sqlx::query_as::<_, TimelineEntry>(
        r#"
        SELECT
            rd.id, rd.created_at, i.symbol, rd.strategy_id, rd.trigger, rd.approved, rd.reason,
            s.direction AS signal_direction, s.confidence AS signal_confidence,
            o.side AS order_side, o.quantity AS order_quantity, o.status AS order_status,
            f.price AS fill_price, f.fee AS fill_fee,
            f.spread_cost AS fill_spread_cost, f.slippage_cost AS fill_slippage_cost,
            t.pnl_gross, t.pnl_net
        FROM risk_decisions rd
        JOIN instruments i ON i.id = rd.instrument_id
        LEFT JOIN signals s ON s.id = rd.signal_id
        LEFT JOIN orders o ON o.id = rd.order_id
        LEFT JOIN fills f ON f.order_id = o.id
        LEFT JOIN trades t ON t.order_id = o.id
        ORDER BY rd.created_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}
