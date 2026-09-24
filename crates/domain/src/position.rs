use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::instrument::InstrumentId;
use crate::side::Side;
use crate::signal::StrategyId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PositionStatus {
    Open,
    Closed,
}

/// Uma posição mantida no portfólio (paper). A posse deste estado é
/// responsabilidade do crate de portfólio — este tipo é o formato de dados
/// compartilhado usado pelas camadas de broker, portfólio e persistência, para
/// que cada uma não invente o seu próprio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub id: Uuid,
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    pub side: Side,
    pub quantity: Decimal,
    pub entry_price: Decimal,
    pub exit_price: Option<Decimal>,
    pub opened_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub status: PositionStatus,
    /// P&L bruto realizado (antes das taxas, mas *após* spread/slippage — estes
    /// estão embutidos nos próprios `entry_price`/`exit_price`), definido apenas
    /// quando `status == Closed`.
    pub realized_pnl_gross: Option<Decimal>,
    /// P&L líquido realizado: `realized_pnl_gross - fees_paid`. Já reflete
    /// spread e slippage (via os preços de entrada/saída que produziram
    /// `realized_pnl_gross`) além das taxas — veja `spread_paid`/
    /// `slippage_paid` para o detalhamento item a item dos primeiros. Definido
    /// apenas quando `status == Closed`.
    pub realized_pnl_net: Option<Decimal>,
    /// Taxas acumuladas nos fills de entrada + saída.
    pub fees_paid: Decimal,
    /// Custo de spread acumulado nos fills de entrada + saída — apenas
    /// diagnóstico. Já refletido em `realized_pnl_gross` via os preços dos
    /// fills; não é uma dedução adicional.
    pub spread_paid: Decimal,
    /// Custo de slippage acumulado nos fills de entrada + saída — apenas
    /// diagnóstico, com a mesma ressalva de `spread_paid`.
    pub slippage_paid: Decimal,
}

impl Position {
    /// P&L não realizado em `mark_price`, bruto de taxas. Só faz sentido
    /// enquanto `status == Open`; os chamadores não devem invocá-lo em uma
    /// posição fechada.
    pub fn unrealized_pnl(&self, mark_price: Decimal) -> Decimal {
        let diff = mark_price - self.entry_price;
        match self.side {
            Side::Buy => diff * self.quantity,
            Side::Sell => -diff * self.quantity,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn open_position(side: Side, entry_price: Decimal, quantity: Decimal) -> Position {
        Position {
            id: Uuid::new_v4(),
            instrument_id: InstrumentId::new(),
            strategy_id: StrategyId::new("test").unwrap(),
            side,
            quantity,
            entry_price,
            exit_price: None,
            opened_at: Utc::now(),
            closed_at: None,
            status: PositionStatus::Open,
            realized_pnl_gross: None,
            realized_pnl_net: None,
            fees_paid: Decimal::ZERO,
            spread_paid: Decimal::ZERO,
            slippage_paid: Decimal::ZERO,
        }
    }

    #[test]
    fn long_unrealized_pnl_is_positive_when_price_rises() {
        let pos = open_position(Side::Buy, dec!(100), dec!(2));
        assert_eq!(pos.unrealized_pnl(dec!(110)), dec!(20));
    }

    #[test]
    fn short_unrealized_pnl_is_positive_when_price_falls() {
        let pos = open_position(Side::Sell, dec!(100), dec!(2));
        assert_eq!(pos.unrealized_pnl(dec!(90)), dec!(20));
    }
}
