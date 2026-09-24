use domain::Money;
use rust_decimal::Decimal;

/// Limites de risco aplicados a toda intenção de ordem antes de ela chegar
/// a um broker. Todos os campos são obrigatórios (não existe "ilimitado"
/// implícito), para que um deploy mal configurado falhe de forma ruidosa na
/// inicialização em vez de operar tamanho ilimitado silenciosamente.
#[derive(Debug, Clone)]
pub struct RiskConfig {
    /// Notional (na moeda de cotação) solicitado para cada nova posição.
    /// Esta é uma regra de dimensionamento por notional fixo,
    /// deliberadamente simples — veja `docs/architecture.md` para entender
    /// por que dimensionamentos mais sofisticados (vol targeting, Kelly,
    /// ...) estão fora do escopo desta fase.
    pub order_notional: Money,
    /// Teto rígido para o notional de qualquer posição individual.
    pub max_position_notional: Money,
    /// Teto rígido para a exposição notional total somando todas as
    /// posições abertas.
    pub max_total_exposure: Money,
    /// Teto rígido para a quantidade de posições abertas simultaneamente.
    pub max_open_positions: usize,
    /// Stop loss fracionário aplicado a toda posição, por exemplo `0.05` =
    /// sair quando a posição estiver 5% abaixo da entrada. `None` desativa.
    pub stop_loss_pct: Option<Decimal>,
    /// Take profit fracionário aplicado a toda posição. `None` desativa.
    pub take_profit_pct: Option<Decimal>,
    /// Quando as perdas realizadas do dia UTC corrente atingem esta
    /// magnitude, novas entradas são rejeitadas (posições existentes ainda
    /// podem ser fechadas).
    pub max_daily_loss: Money,
}
