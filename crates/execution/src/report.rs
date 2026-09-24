use domain::{Fill, Order, Position};

/// O que aconteceu com o portfólio como resultado de uma ordem executada:
/// ou uma posição totalmente nova foi aberta, ou uma existente foi fechada
/// por completo. O `PaperBroker` nunca fecha parcialmente uma posição nesta
/// fase.
#[derive(Debug, Clone, PartialEq)]
pub enum PositionEvent {
    Opened(Position),
    Closed(Position),
}

/// O resultado de uma ordem executada com sucesso: o registro `Order`
/// atualizado, o `Fill` ocorrido e a mudança resultante nas posições do
/// portfólio.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionReport {
    pub order: Order,
    pub fill: Fill,
    pub position_event: PositionEvent,
}
