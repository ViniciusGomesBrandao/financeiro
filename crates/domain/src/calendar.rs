use chrono::{DateTime, Utc};

/// Indica se o mercado de um instrumento está aberto para negociação em um dado
/// instante.
///
/// Existe para que "o mercado está aberto?" seja respondido em exatamente um
/// lugar por mercado, em vez de ser reimplementado (ou esquecido) dentro de cada
/// estratégia. Cripto usa `AlwaysOpenCalendar`; ações eventualmente fornecerão
/// `B3TradingCalendar`, `NyseTradingCalendar`, etc. em um crate/módulo
/// específico de ações — veja `docs/architecture.md` para o plano.
///
/// Estratégias nunca devem consultar o relógio por conta própria para decidir se
/// negociam; essa responsabilidade é de quem conduz a estratégia (o pipeline ao
/// vivo ou o runner de backtest), consultando um `TradingCalendar`.
pub trait TradingCalendar: Send + Sync {
    fn is_open(&self, at: DateTime<Utc>) -> bool;
}

/// Um mercado sem sessões: está sempre aberto. Correto para cripto
/// spot/perpétuo, que negocia 24/7.
#[derive(Debug, Clone, Copy, Default)]
pub struct AlwaysOpenCalendar;

impl TradingCalendar for AlwaysOpenCalendar {
    fn is_open(&self, _at: DateTime<Utc>) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_open_calendar_is_always_open() {
        let cal = AlwaysOpenCalendar;
        assert!(cal.is_open(Utc::now()));
    }
}
