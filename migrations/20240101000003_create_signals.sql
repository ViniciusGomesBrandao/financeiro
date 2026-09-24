-- Todo sinal emitido por uma estratégia, independentemente de o risk engine
-- aprovar ou não uma order a partir dele. É isso que torna o comportamento
-- das estratégias auditável depois do fato.
CREATE TABLE signals (
    id                  UUID PRIMARY KEY,
    strategy_id         TEXT NOT NULL REFERENCES strategy_configs (id),
    instrument_id       UUID NOT NULL REFERENCES instruments (id),
    direction           TEXT NOT NULL,
    confidence          DOUBLE PRECISION NOT NULL,
    expected_return     NUMERIC,
    time_horizon        TEXT,
    metadata            JSONB NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_signals_strategy_id ON signals (strategy_id);
CREATE INDEX idx_signals_instrument_id ON signals (instrument_id);
CREATE INDEX idx_signals_created_at ON signals (created_at);
