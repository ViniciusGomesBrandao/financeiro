use domain::{Money, OrderRequest};

/// Por que o motor de risco se recusou a transformar um sinal em ordem.
/// Cada variante carrega os números envolvidos, para que a rejeição seja
/// autoexplicativa em logs e na persistência, e não apenas um rótulo seco.
#[derive(Debug, Clone, PartialEq)]
pub enum RejectionReason {
    /// Chegou um sinal `Flat` para um instrumento sem posição aberta — não
    /// há nada a fechar.
    NothingToFlatten,
    /// Chegou um sinal `Short` para um instrumento sem posição aberta.
    /// Contas spot não têm mecanismo de empréstimo/margem, então não há
    /// nada a vender e nenhuma posição short pode ser aberta. Este é o
    /// comportamento esperado e rotineiro para um sinal de baixa estando
    /// zerado — não é uma condição de erro.
    ShortSellingNotSupported,
    /// Chegou um sinal direcional para um instrumento que já tem posição
    /// aberta. Este motor não suporta pirâmide nem reversão no lugar; um
    /// sinal `Flat` precisa fechar a posição existente primeiro.
    PositionAlreadyOpen,
    MaxOpenPositionsReached {
        current: usize,
        max: usize,
    },
    PositionSizeExceeded {
        requested: Money,
        max: Money,
    },
    ExposureLimitExceeded {
        projected: Money,
        max: Money,
    },
    InsufficientBalance {
        required: Money,
        available: Money,
    },
    DailyLossLimitBreached {
        realized_today: Money,
        limit: Money,
    },
    BelowExchangeMinimum {
        requested: Money,
        minimum: Money,
    },
}

impl std::fmt::Display for RejectionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RejectionReason::NothingToFlatten => write!(f, "no open position to flatten"),
            RejectionReason::ShortSellingNotSupported => write!(
                f,
                "short selling is not supported on spot; no position to sell"
            ),
            RejectionReason::PositionAlreadyOpen => {
                write!(f, "position already open for this instrument")
            }
            RejectionReason::MaxOpenPositionsReached { current, max } => {
                write!(f, "max open positions reached ({current}/{max})")
            }
            RejectionReason::PositionSizeExceeded { requested, max } => {
                write!(f, "position size {requested} exceeds max {max}")
            }
            RejectionReason::ExposureLimitExceeded { projected, max } => {
                write!(f, "projected exposure {projected} exceeds max {max}")
            }
            RejectionReason::InsufficientBalance {
                required,
                available,
            } => write!(f, "insufficient balance: need {required}, have {available}"),
            RejectionReason::DailyLossLimitBreached {
                realized_today,
                limit,
            } => write!(
                f,
                "daily loss limit breached: realized {realized_today}, limit {limit}"
            ),
            RejectionReason::BelowExchangeMinimum { requested, minimum } => write!(
                f,
                "order notional {requested} below exchange minimum {minimum}"
            ),
        }
    }
}

/// O veredito do motor de risco sobre um sinal: ou um `OrderRequest` pronto
/// para ser entregue a um broker, ou uma rejeição com motivo explícito. Não
/// existe um terceiro desfecho de "ignorar silenciosamente".
#[derive(Debug, Clone, PartialEq)]
pub enum RiskDecision {
    Approved(OrderRequest),
    Rejected(RejectionReason),
}

impl RiskDecision {
    pub fn is_approved(&self) -> bool {
        matches!(self, RiskDecision::Approved(_))
    }
}

/// Por que uma posição aberta foi fechada pelo monitoramento de risco, e
/// não por um novo sinal da estratégia.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    StopLoss,
    TakeProfit,
}
