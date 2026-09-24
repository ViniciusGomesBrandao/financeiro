use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::instrument::InstrumentId;
use crate::side::Side;
use crate::signal::{SignalId, StrategyId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderType {
    Market,
    Limit,
}

/// Uma intenção de ordem produzida pelo motor de risco após aprovar um sinal.
/// É isto que é entregue a um `Broker` — ainda *não* foi executada.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderRequest {
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    /// O sinal que originou esta ordem, quando houve um. Saídas motivadas por
    /// risco (stop loss / take profit / limite de perda diária) podem originar
    /// uma requisição de ordem sem sinal.
    pub signal_id: Option<SignalId>,
    pub side: Side,
    pub order_type: OrderType,
    pub quantity: Decimal,
    /// Obrigatório para `OrderType::Limit`, ignorado para `OrderType::Market`.
    pub limit_price: Option<Decimal>,
    pub requested_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Rejected,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Liquidity {
    Maker,
    Taker,
}

/// Um único evento de fill contra uma `Order`. Uma ordem a mercado num paper
/// broker normalmente produz exatamente um fill; o tipo permite mais sem forçar
/// todo consumidor a lidar com fills parciais hoje.
///
/// `fee` é o único custo que *não* está já embutido em `price` — é uma dedução
/// de caixa separada. `spread_cost` e `slippage_cost` são a magnitude notional
/// (na moeda de cotação) de cada ajuste adverso de preço que *foi* aplicado a
/// `price`, destacados aqui apenas para que o custo total simulado de negociação
/// de um fill (`fee + spread_cost + slippage_cost`) possa ser consultado e
/// reportado por componente. São diagnósticos, não uma dedução adicional:
/// `price` já reflete ambos, portanto o P&L calculado a partir de diferenças de
/// `price` já os desconta — veja a documentação do módulo
/// `execution::PaperBroker`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fill {
    pub id: Uuid,
    pub order_id: Uuid,
    pub price: Decimal,
    pub quantity: Decimal,
    pub fee: Decimal,
    pub spread_cost: Decimal,
    pub slippage_cost: Decimal,
    pub liquidity: Liquidity,
    pub executed_at: DateTime<Utc>,
}

/// O registro persistido de uma ordem e de seu ciclo de vida. `OrderRequest` é
/// a intenção; `Order` é o resultado acompanhado depois que um broker agiu (ou
/// não) sobre ela.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Order {
    pub id: Uuid,
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    pub signal_id: Option<SignalId>,
    pub side: Side,
    pub order_type: OrderType,
    pub quantity: Decimal,
    pub limit_price: Option<Decimal>,
    pub status: OrderStatus,
    pub filled_quantity: Decimal,
    pub average_fill_price: Option<Decimal>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Order {
    pub fn new(request: &OrderRequest) -> Self {
        Self {
            id: Uuid::new_v4(),
            instrument_id: request.instrument_id,
            strategy_id: request.strategy_id.clone(),
            signal_id: request.signal_id,
            side: request.side,
            order_type: request.order_type,
            quantity: request.quantity,
            limit_price: request.limit_price,
            status: OrderStatus::New,
            filled_quantity: Decimal::ZERO,
            average_fill_price: None,
            created_at: request.requested_at,
            updated_at: request.requested_at,
        }
    }
}
