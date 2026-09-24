use chrono::{DateTime, Utc};
use domain::{InstrumentId, Position, Side, StrategyId};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::codec::{side_from_str, side_to_str};
use crate::error::PersistenceError;

/// Um trade completo de ida e volta, derivado 1:1 de uma `Position`
/// fechada. Não é um tipo de `domain`: é um lançamento contábil da camada de
/// persistência, mantido separado de `positions` (ver
/// `migrations/..._create_trades.sql`) para queries de analytics que só se
/// interessam pelo histórico já fechado.
///
/// `spread_paid`/`slippage_paid` espelham os campos de mesmo nome de
/// `Position`: são detalhamentos diagnósticos de um custo já embutido em
/// `entry_price`/`exit_price`, não uma dedução adicional do `pnl_net`.
#[derive(Debug, Clone)]
pub struct ClosedTradeRecord {
    pub id: Uuid,
    pub position_id: Uuid,
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    pub side: Side,
    pub quantity: Decimal,
    pub entry_price: Decimal,
    pub exit_price: Decimal,
    pub opened_at: DateTime<Utc>,
    pub closed_at: DateTime<Utc>,
    pub pnl_gross: Decimal,
    pub pnl_net: Decimal,
    pub fees_paid: Decimal,
    pub spread_paid: Decimal,
    pub slippage_paid: Decimal,
    /// A order que fechou este trade — permite à UI de observabilidade
    /// juntar a timeline de decisões (`risk_decisions`) ao P&L resultante.
    pub order_id: Option<Uuid>,
}

impl ClosedTradeRecord {
    /// Monta um registro a partir de uma `Position` que acabou de ser
    /// fechada. Retorna `None` se `position` não estiver de fato fechada —
    /// espera-se que os chamadores invoquem isto exatamente uma vez por
    /// fechamento, logo após `PortfolioManager::close_position` retornar.
    pub fn from_closed_position(position: &Position, order_id: Uuid) -> Option<Self> {
        let exit_price = position.exit_price?;
        let closed_at = position.closed_at?;
        let pnl_gross = position.realized_pnl_gross?;
        let pnl_net = position.realized_pnl_net?;
        Some(Self {
            id: Uuid::new_v4(),
            position_id: position.id,
            instrument_id: position.instrument_id,
            strategy_id: position.strategy_id.clone(),
            side: position.side,
            quantity: position.quantity,
            entry_price: position.entry_price,
            exit_price,
            opened_at: position.opened_at,
            closed_at,
            pnl_gross,
            pnl_net,
            fees_paid: position.fees_paid,
            spread_paid: position.spread_paid,
            slippage_paid: position.slippage_paid,
            order_id: Some(order_id),
        })
    }
}

#[derive(sqlx::FromRow)]
struct TradeRow {
    id: Uuid,
    position_id: Uuid,
    instrument_id: Uuid,
    strategy_id: String,
    side: String,
    quantity: Decimal,
    entry_price: Decimal,
    exit_price: Decimal,
    opened_at: DateTime<Utc>,
    closed_at: DateTime<Utc>,
    pnl_gross: Decimal,
    pnl_net: Decimal,
    fees_paid: Decimal,
    spread_paid: Decimal,
    slippage_paid: Decimal,
    order_id: Option<Uuid>,
}

fn row_to_record(row: TradeRow) -> Result<ClosedTradeRecord, PersistenceError> {
    Ok(ClosedTradeRecord {
        id: row.id,
        position_id: row.position_id,
        instrument_id: InstrumentId(row.instrument_id),
        strategy_id: StrategyId::new(row.strategy_id)
            .map_err(|e| PersistenceError::Database(sqlx::Error::Decode(e.to_string().into())))?,
        side: side_from_str(&row.side)?,
        quantity: row.quantity,
        entry_price: row.entry_price,
        exit_price: row.exit_price,
        opened_at: row.opened_at,
        closed_at: row.closed_at,
        pnl_gross: row.pnl_gross,
        pnl_net: row.pnl_net,
        fees_paid: row.fees_paid,
        spread_paid: row.spread_paid,
        slippage_paid: row.slippage_paid,
        order_id: row.order_id,
    })
}

pub async fn insert(pool: &PgPool, trade: &ClosedTradeRecord) -> Result<(), PersistenceError> {
    sqlx::query(
        r#"
        INSERT INTO trades
            (id, position_id, instrument_id, strategy_id, side, quantity, entry_price,
             exit_price, opened_at, closed_at, pnl_gross, pnl_net, fees_paid,
             spread_paid, slippage_paid, order_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
        "#,
    )
    .bind(trade.id)
    .bind(trade.position_id)
    .bind(trade.instrument_id.0)
    .bind(trade.strategy_id.as_str())
    .bind(side_to_str(trade.side))
    .bind(trade.quantity)
    .bind(trade.entry_price)
    .bind(trade.exit_price)
    .bind(trade.opened_at)
    .bind(trade.closed_at)
    .bind(trade.pnl_gross)
    .bind(trade.pnl_net)
    .bind(trade.fees_paid)
    .bind(trade.spread_paid)
    .bind(trade.slippage_paid)
    .bind(trade.order_id)
    .execute(pool)
    .await?;
    Ok(())
}

const SELECT_COLUMNS: &str = r#"id, position_id, instrument_id, strategy_id, side, quantity,
    entry_price, exit_price, opened_at, closed_at, pnl_gross, pnl_net, fees_paid,
    spread_paid, slippage_paid, order_id"#;

pub async fn list_for_strategy(
    pool: &PgPool,
    strategy_id: &StrategyId,
) -> Result<Vec<ClosedTradeRecord>, PersistenceError> {
    let query =
        format!("SELECT {SELECT_COLUMNS} FROM trades WHERE strategy_id = $1 ORDER BY closed_at");
    let rows: Vec<TradeRow> = sqlx::query_as(&query)
        .bind(strategy_id.as_str())
        .fetch_all(pool)
        .await?;

    rows.into_iter().map(row_to_record).collect()
}

/// Todos os trades fechados, de todas as estratégias — usado pela UI de
/// observabilidade para o histórico geral e para o cálculo de drawdown
/// (via `analytics::compute_performance`) sobre a carteira inteira.
pub async fn list_all(pool: &PgPool) -> Result<Vec<ClosedTradeRecord>, PersistenceError> {
    let query = format!("SELECT {SELECT_COLUMNS} FROM trades ORDER BY closed_at DESC");
    let rows: Vec<TradeRow> = sqlx::query_as(&query).fetch_all(pool).await?;

    rows.into_iter().map(row_to_record).collect()
}
