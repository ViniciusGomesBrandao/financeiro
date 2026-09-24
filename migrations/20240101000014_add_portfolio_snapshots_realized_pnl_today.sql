-- Persiste o P&L realizado do dia UTC corrente (já calculado internamente
-- por PortfolioManager::realized_pnl_today para a checagem de limite de
-- perda diária do Risk Engine) para que a UI de observabilidade mostre
-- "P&L diário" sem reimplementar esse cálculo em outra camada.
ALTER TABLE portfolio_snapshots
    ADD COLUMN realized_pnl_today NUMERIC NOT NULL DEFAULT 0;
