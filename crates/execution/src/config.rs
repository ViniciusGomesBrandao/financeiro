use rust_decimal::Decimal;

/// Modelo de fee, spread e slippage do `PaperBroker`. Todo fill é cobrado
/// com `taker_fee` (veja a documentação do módulo `PaperBroker` para
/// entender por que fills maker ainda não são modelados) e tem seu preço
/// afastado do preço de referência por `spread_bps` e `slippage_bps`,
/// aplicados como dois ajustes adversos distintos — a execução
/// deliberadamente nunca é "gratuita e perfeita".
///
/// `spread_bps` e `slippage_bps` são modelados e registrados separadamente
/// (veja `Fill::spread_cost` / `Fill::slippage_cost`) mesmo que ambos sejam
/// aplicados da mesma forma (sempre adversos, proporcionais ao preço de
/// referência): eles representam custos conceitualmente diferentes —
/// `spread_bps` é o custo de cruzar o bid/ask como taker, `slippage_bps` é
/// movimento de preço/impacto de mercado adicional — e mantê-los distintos
/// é o que permite consultar fills persistidos separadamente depois, em vez
/// de um único número agregado de "slippage" que silenciosamente mistura os
/// dois.
#[derive(Debug, Clone, Copy)]
pub struct PaperBrokerConfig {
    /// Fração do notional cobrada em um fill maker, ex.: `0.001` = 0,1%.
    /// Reservado para quando a simulação de ordens limit/maker for
    /// implementada.
    pub maker_fee: Decimal,
    /// Fração do notional cobrada em um fill taker, ex.: `0.001` = 0,1%.
    pub taker_fee: Decimal,
    /// Custo de spread bid/ask aplicado a todo fill, em basis points do
    /// preço de referência (1 bps = 0,01%). Sempre adverso: compras
    /// executam mais caro, vendas executam mais barato.
    pub spread_bps: Decimal,
    /// Slippage adicional aplicado a todo fill, além de `spread_bps`, nas
    /// mesmas unidades e direção.
    pub slippage_bps: Decimal,
}
