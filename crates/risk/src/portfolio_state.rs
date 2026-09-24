use domain::{InstrumentId, Money, Position, StrategyId};

/// Um snapshot somente leitura do portfólio, tal como o motor de risco
/// precisa para avaliar um sinal. O crate `portfolio` é o dono do estado
/// mutável de verdade (caixa, posições) e é responsável por produzir este
/// snapshot sob demanda — o motor de risco nunca altera o estado do
/// portfólio diretamente, e o portfólio nunca aplica regras de risco por
/// conta própria. Este tipo é intencionalmente um snapshot simples em vez
/// de uma trait: assim evita-se uma dependência entre os crates `risk` e
/// `portfolio` em qualquer direção.
#[derive(Debug, Clone)]
pub struct PortfolioState<'a> {
    pub cash: Money,
    pub equity: Money,
    pub open_positions: &'a [Position],
    /// P&L realizado (líquido) do dia UTC corrente, negativo em caso de
    /// prejuízo líquido.
    pub realized_pnl_today: Money,
}

impl<'a> PortfolioState<'a> {
    /// A posição aberta de `strategy_id` (o "robô") em `instrument_id`, se
    /// houver — nunca a posição de outro robô no mesmo instrumento (ver
    /// ADR-20: "posição aberta" é uma propriedade do par
    /// (instrumento, robô), não só do instrumento).
    pub fn open_position_for(
        &self,
        instrument_id: InstrumentId,
        strategy_id: &StrategyId,
    ) -> Option<&Position> {
        self.open_positions
            .iter()
            .find(|p| p.instrument_id == instrument_id && &p.strategy_id == strategy_id)
    }

    /// Exposição total somando as posições abertas, aproximada pelo
    /// notional de entrada (`entry_price * quantity`) em vez do notional a
    /// mercado. Esta é uma simplificação: a exposição exata ao vivo exigiria
    /// um preço atual para cada instrumento aberto, algo que o motor de
    /// risco não tem no momento da avaliação. Documentado aqui em vez de
    /// escondido.
    pub fn total_exposure(&self) -> Money {
        self.open_positions.iter().fold(Money::zero(), |acc, p| {
            acc + Money::new(p.entry_price * p.quantity)
        })
    }

    pub fn open_position_count(&self) -> usize {
        self.open_positions.len()
    }
}
