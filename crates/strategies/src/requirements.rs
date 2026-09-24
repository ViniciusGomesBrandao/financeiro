use domain::{AssetClass, MarketDataKind};

/// O contrato declarado de uma estratégia: quais dados de mercado ela precisa e
/// contra quais classes de ativos é válido executá-la.
///
/// Este é todo o mecanismo que o sistema usa para impedir que uma estratégia
/// rode onde não faz sentido — veja `registry::check_compatible`. Uma estratégia
/// que omite ou subdeclara seus requisitos pode ser registrada silenciosamente
/// contra um mercado incompatível, portanto toda implementação de `Strategy`
/// deve retornar um `StrategyRequirements` honesto e completo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyRequirements {
    pub required_market_data: Vec<MarketDataKind>,
    pub supported_asset_classes: Vec<AssetClass>,
}

impl StrategyRequirements {
    pub fn new(
        required_market_data: Vec<MarketDataKind>,
        supported_asset_classes: Vec<AssetClass>,
    ) -> Self {
        Self {
            required_market_data,
            supported_asset_classes,
        }
    }

    pub fn supports(&self, asset_class: AssetClass) -> bool {
        self.supported_asset_classes.contains(&asset_class)
    }

    pub fn requires(&self, kind: MarketDataKind) -> bool {
        self.required_market_data.contains(&kind)
    }
}
