-- Referencia a order que fechou o trade, para permitir reconstruir a
-- timeline "sinal -> motivo -> decisão -> execução -> fechamento/P&L" com
-- um único join a partir de risk_decisions, sem heurística de correlação
-- por instrumento/horário.
ALTER TABLE trades
    ADD COLUMN order_id UUID REFERENCES orders (id);
