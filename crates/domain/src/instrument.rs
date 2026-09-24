use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::asset::{Asset, Symbol};
use crate::asset_class::{AssetClass, Exchange, MarketType};

/// Identidade estável de um instrumento, independente de seu símbolo legível.
/// Usada como chave estrangeira em todo o resto do domínio (candles, trades,
/// ordens, posições) para que a renomeação de um símbolo nunca se propague.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstrumentId(pub Uuid);

impl InstrumentId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for InstrumentId {
    fn default() -> Self {
        Self::new()
    }
}

/// Tudo o que o resto do sistema precisa saber sobre um instrumento negociável,
/// independente do formato de transmissão de qualquer exchange específica.
///
/// `tick_size` / `lot_size` / `min_quantity` / `min_notional` existem para que a
/// validação de ordens (risco, paper broker) nunca precise tratar uma exchange
/// como caso especial: o adaptador da exchange é responsável por traduzir seus
/// próprios metadados para estes campos uma única vez, no momento do registro do
/// instrumento.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Instrument {
    pub id: InstrumentId,
    pub symbol: Symbol,
    pub base_asset: Asset,
    pub quote_asset: Asset,
    pub asset_class: AssetClass,
    pub exchange: Exchange,
    pub market_type: MarketType,
    /// Incremento mínimo de preço.
    pub tick_size: Decimal,
    /// Incremento mínimo de quantidade.
    pub lot_size: Decimal,
    pub min_quantity: Decimal,
    pub min_notional: Decimal,
}

impl Instrument {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        symbol: Symbol,
        base_asset: Asset,
        quote_asset: Asset,
        asset_class: AssetClass,
        exchange: Exchange,
        market_type: MarketType,
        tick_size: Decimal,
        lot_size: Decimal,
        min_quantity: Decimal,
        min_notional: Decimal,
    ) -> Self {
        Self {
            id: InstrumentId::new(),
            symbol,
            base_asset,
            quote_asset,
            asset_class,
            exchange,
            market_type,
            tick_size,
            lot_size,
            min_quantity,
            min_notional,
        }
    }

    /// Arredonda `price` para o múltiplo válido mais próximo de `tick_size` — o
    /// único lugar onde o dimensionamento/execução de ordens deve quantizar um
    /// preço, para que os chamadores (`risk`, `execution`) nunca reimplementem
    /// cada um a matemática de tick-size da exchange. Um `tick_size` não
    /// positivo (instrumento mal configurado) é tratado como "sem restrição":
    /// `price` é retornado inalterado em vez de dividir por zero.
    pub fn round_price_to_tick(&self, price: Decimal) -> Decimal {
        if self.tick_size <= Decimal::ZERO {
            return price;
        }
        (price / self.tick_size).round() * self.tick_size
    }

    /// Arredonda `quantity` *para baixo* até o múltiplo válido mais próximo de
    /// `lot_size`. Sempre arredonda para baixo, nunca para cima: arredondar para
    /// cima poderia levar uma ordem a exceder o notional aprovado pelo risco ou
    /// o caixa realmente disponível, o que arredondar para baixo nunca faz. Um
    /// `lot_size` não positivo é tratado como "sem restrição": `quantity` é
    /// retornado inalterado.
    pub fn round_quantity_down_to_lot(&self, quantity: Decimal) -> Decimal {
        if self.lot_size <= Decimal::ZERO {
            return quantity;
        }
        (quantity / self.lot_size).floor() * self.lot_size
    }
}

#[cfg(test)]
mod instrument_tests {
    use super::*;
    use crate::asset_class::{AssetClass, Exchange, MarketType};
    use rust_decimal_macros::dec;

    fn instrument(tick_size: Decimal, lot_size: Decimal) -> Instrument {
        let base = Asset::new("BTC").unwrap();
        let quote = Asset::new("USDT").unwrap();
        Instrument::new(
            Symbol::from_pair(&base, &quote),
            base,
            quote,
            AssetClass::Crypto,
            Exchange::Binance,
            MarketType::Spot,
            tick_size,
            lot_size,
            dec!(0.0001),
            dec!(10),
        )
    }

    #[test]
    fn rounds_price_to_nearest_tick() {
        let instrument = instrument(dec!(0.01), dec!(0.0001));
        assert_eq!(
            instrument.round_price_to_tick(dec!(50025.003)),
            dec!(50025.00)
        );
        assert_eq!(
            instrument.round_price_to_tick(dec!(50025.007)),
            dec!(50025.01)
        );
        // Já exato: inalterado.
        assert_eq!(
            instrument.round_price_to_tick(dec!(50025.00)),
            dec!(50025.00)
        );
    }

    #[test]
    fn rounds_quantity_down_to_lot_never_up() {
        let instrument = instrument(dec!(0.01), dec!(0.0001));
        // 0.01149999 / 0.0001 = 114.9999 -> piso 114 -> 0.0114
        assert_eq!(
            instrument.round_quantity_down_to_lot(dec!(0.01149999)),
            dec!(0.0114)
        );
        // Já exato: inalterado.
        assert_eq!(
            instrument.round_quantity_down_to_lot(dec!(0.02)),
            dec!(0.02)
        );
    }

    #[test]
    fn non_positive_tick_or_lot_size_is_treated_as_unconstrained() {
        let instrument = instrument(Decimal::ZERO, Decimal::ZERO);
        assert_eq!(instrument.round_price_to_tick(dec!(123.456)), dec!(123.456));
        assert_eq!(
            instrument.round_quantity_down_to_lot(dec!(0.123456)),
            dec!(0.123456)
        );
    }
}
