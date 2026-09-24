-- Registro de toda decisão do Risk Engine, aprovada ou rejeitada — inclui
-- o motivo. Sem esta tabela não é possível reconstruir "sinal -> motivo ->
-- decisão" para sinais rejeitados: eles nunca geram uma order, então não
-- deixam nenhum rastro nas tabelas existentes.
--
-- `trigger` distingue a origem da decisão:
--   'signal'      -> avaliação normal de um sinal de estratégia
--   'stop_loss'   -> saída forçada pelo Risk Engine (stop loss)
--   'take_profit' -> saída forçada pelo Risk Engine (take profit)
-- Saídas forçadas não têm signal_id (não vêm de uma estratégia).
CREATE TABLE risk_decisions (
    id              UUID PRIMARY KEY,
    signal_id       UUID REFERENCES signals (id),
    instrument_id   UUID NOT NULL REFERENCES instruments (id),
    strategy_id     TEXT NOT NULL REFERENCES strategy_configs (id),
    trigger         TEXT NOT NULL,
    approved        BOOLEAN NOT NULL,
    reason          TEXT,
    order_id        UUID REFERENCES orders (id),
    created_at      TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_risk_decisions_strategy_id ON risk_decisions (strategy_id);
CREATE INDEX idx_risk_decisions_instrument_id ON risk_decisions (instrument_id);
CREATE INDEX idx_risk_decisions_created_at ON risk_decisions (created_at);
