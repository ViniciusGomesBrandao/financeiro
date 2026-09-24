use std::fmt;
use std::ops::{Add, Sub};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// Um preço estritamente positivo. Encapsular `Decimal` (nunca `f32`/`f64`)
/// mantém os preços exatos e evita, no nível de tipos, misturá-los
/// acidentalmente com quantidades ou valores brutos de caixa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Price(Decimal);

impl Price {
    pub fn new(value: Decimal) -> Result<Self, DomainError> {
        if value <= Decimal::ZERO {
            return Err(DomainError::NonPositivePrice(value.to_string()));
        }
        Ok(Self(value))
    }

    pub fn value(&self) -> Decimal {
        self.0
    }
}

impl fmt::Display for Price {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Uma quantidade estritamente positiva do ativo base.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Quantity(Decimal);

impl Quantity {
    pub fn new(value: Decimal) -> Result<Self, DomainError> {
        if value <= Decimal::ZERO {
            return Err(DomainError::NonPositiveQuantity(value.to_string()));
        }
        Ok(Self(value))
    }

    pub fn value(&self) -> Decimal {
        self.0
    }
}

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Valor notional: `Price * Quantity`, ou qualquer outro montante na moeda de
/// cotação (caixa, taxas, P&L). Baseado em `Decimal`; pode ser negativo (ex. um
/// prejuízo).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Money(Decimal);

impl Money {
    pub fn new(value: Decimal) -> Self {
        Self(value)
    }

    pub fn zero() -> Self {
        Self(Decimal::ZERO)
    }

    pub fn value(&self) -> Decimal {
        self.0
    }

    pub fn is_negative(&self) -> bool {
        self.0.is_sign_negative() && !self.0.is_zero()
    }
}

impl From<Price> for Money {
    fn from(price: Price) -> Self {
        Money(price.0)
    }
}

impl Add for Money {
    type Output = Money;
    fn add(self, rhs: Self) -> Self::Output {
        Money(self.0 + rhs.0)
    }
}

impl Sub for Money {
    type Output = Money;
    fn sub(self, rhs: Self) -> Self::Output {
        Money(self.0 - rhs.0)
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Valor notional de um fill: `price * quantity`.
pub fn notional(price: Price, quantity: Quantity) -> Money {
    Money(price.value() * quantity.value())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn price_rejects_non_positive() {
        assert!(Price::new(dec!(0)).is_err());
        assert!(Price::new(dec!(-1)).is_err());
        assert!(Price::new(dec!(0.01)).is_ok());
    }

    #[test]
    fn quantity_rejects_non_positive() {
        assert!(Quantity::new(dec!(0)).is_err());
        assert!(Quantity::new(dec!(-1)).is_err());
    }

    #[test]
    fn notional_multiplies_price_by_quantity() {
        let price = Price::new(dec!(50000)).unwrap();
        let qty = Quantity::new(dec!(0.1)).unwrap();
        assert_eq!(notional(price, qty), Money::new(dec!(5000)));
    }

    #[test]
    fn money_supports_add_sub() {
        let a = Money::new(dec!(100));
        let b = Money::new(dec!(30));
        assert_eq!(a + b, Money::new(dec!(130)));
        assert_eq!(a - b, Money::new(dec!(70)));
    }
}
