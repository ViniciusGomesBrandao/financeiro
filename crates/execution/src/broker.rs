use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::{Instrument, OrderRequest, Price};
use portfolio::PortfolioManager;

use crate::error::BrokerError;
use crate::report::ExecutionReport;

/// Executa intenções de ordem contra alguma venue.
///
/// A única implementação nesta fase é `PaperBroker` — não existe caminho de
/// execução com dinheiro real. Um futuro broker live implementaria este
/// mesmo trait contra a API de envio de ordens de uma exchange real; a
/// assinatura `async fn` existe por esse motivo, ainda que o próprio
/// `PaperBroker` não precise de I/O hoje.
///
/// `portfolio` é passado como parâmetro em vez de pertencer ao broker: o
/// estado do portfólio deve continuar sendo responsabilidade exclusiva de
/// `portfolio::PortfolioManager` (compartilhado com persistência e
/// analytics), não duplicado dentro do broker que estiver configurado.
///
/// `instrument` é necessário para que uma implementação possa quantizar
/// preço e quantidade ao `tick_size`/`lot_size` do instrumento antes de
/// executar — `risk::RiskEngine` já arredonda a quantidade para baixo até o
/// lot size ao dimensionar uma ordem, mas o broker não confia nisso como
/// sua única linha de defesa (espelhando o modo como ele recusa de forma
/// independente um naked short — veja `docs/architecture.md` ADR-8).
///
/// `request.requested_at` e `execution_time` são deliberadamente dois
/// campos diferentes, não um só reaproveitado duas vezes: o primeiro é
/// quando a decisão foi tomada (o timestamp do sinal, ou do candle que
/// disparou uma saída de risco); o segundo é quando o broker de fato agiu
/// sobre ela. Podem coincidir (o caminho ao vivo hoje passa os dois iguais,
/// preservando o comportamento existente) ou divergir (o backtest passa o
/// open() do próximo candle disponível como `execution_time` — ver ADR-18).
/// Uma implementação deve usar `request.requested_at` para `Order::created_at`
/// (via `Order::new`) e `execution_time` para `Fill::executed_at`/
/// `Order::updated_at`/os timestamps de posição — nunca confundir os dois.
#[async_trait]
pub trait Broker: Send {
    async fn submit_order(
        &mut self,
        request: OrderRequest,
        reference_price: Price,
        execution_time: DateTime<Utc>,
        instrument: &Instrument,
        portfolio: &mut PortfolioManager,
    ) -> Result<ExecutionReport, BrokerError>;
}
