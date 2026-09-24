use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Os limiares que definem "economicamente viável" — todos explícitos,
/// nenhum inferido por otimização/tuning. `Default` existe por
/// conveniência (testes, protótipos), não como uma recomendação de
/// produção — assim como `risk::RiskConfig` é sempre construído
/// explicitamente em `app::config`, um deploy real deve escolher estes
/// valores deliberadamente.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JudgeThresholds {
    /// Trades fechados mínimos antes de o Judge opinar sobre viabilidade.
    /// Abaixo disso, a decisão é sempre `Disabled` /
    /// `InsufficientSample`, nunca uma extrapolação otimista.
    pub min_sample_size: usize,
    /// `win_rate` mínimo (a métrica de "consistência").
    pub min_win_rate: Decimal,
    /// `profit_factor` mínimo — ignorado (nunca reprova) quando não há
    /// trades perdedores na amostra (`profit_factor == None`).
    pub min_profit_factor: Decimal,
    /// `max_drawdown` máximo tolerado (valor absoluto, mesma unidade
    /// monetária de `risk::RiskConfig`).
    pub max_drawdown: Decimal,
    /// Quantas avaliações consecutivas apontando para o mesmo novo
    /// estado são exigidas antes de o Judge efetivamente trocar de
    /// recomendação — o mecanismo anti-ruído.
    pub min_confirmations: u32,
}

impl Default for JudgeThresholds {
    fn default() -> Self {
        Self {
            min_sample_size: 10,
            min_win_rate: dec!(0.35),
            min_profit_factor: dec!(1.2),
            max_drawdown: dec!(500),
            min_confirmations: 3,
        }
    }
}
