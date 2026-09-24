-- Separa a fee do spread e do slippage como custos distintos e
-- individualmente consultáveis, em vez de deixar spread/slippage refletidos
-- apenas implicitamente em `fills.price` / `positions.entry_price` /
-- `positions.exit_price`. Ver as docs do módulo `execution::PaperBroker`
-- para o motivo de estes serem detalhamentos diagnósticos de um custo já
-- embutido no preço, e não uma dedução adicional — o P&L realizado não é
-- afetado por esta migration.
ALTER TABLE fills
    ADD COLUMN spread_cost   NUMERIC NOT NULL DEFAULT 0,
    ADD COLUMN slippage_cost NUMERIC NOT NULL DEFAULT 0;

ALTER TABLE positions
    ADD COLUMN spread_paid   NUMERIC NOT NULL DEFAULT 0,
    ADD COLUMN slippage_paid NUMERIC NOT NULL DEFAULT 0;

ALTER TABLE trades
    ADD COLUMN spread_paid   NUMERIC NOT NULL DEFAULT 0,
    ADD COLUMN slippage_paid NUMERIC NOT NULL DEFAULT 0;
