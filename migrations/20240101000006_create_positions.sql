-- Estado atual e histórico das posições, espelhando o estado em memória do
-- `portfolio::PortfolioManager`. Linhas com `status = 'open'` são o book
-- vivo; linhas `'closed'` são histórico.
CREATE TABLE positions (
    id                  UUID PRIMARY KEY,
    instrument_id       UUID NOT NULL REFERENCES instruments (id),
    strategy_id         TEXT NOT NULL REFERENCES strategy_configs (id),
    side                TEXT NOT NULL,
    quantity            NUMERIC NOT NULL,
    entry_price         NUMERIC NOT NULL,
    exit_price          NUMERIC,
    opened_at           TIMESTAMPTZ NOT NULL,
    closed_at           TIMESTAMPTZ,
    status              TEXT NOT NULL,
    realized_pnl_gross  NUMERIC,
    realized_pnl_net    NUMERIC,
    fees_paid           NUMERIC NOT NULL
);

CREATE INDEX idx_positions_instrument_id ON positions (instrument_id);
CREATE INDEX idx_positions_strategy_id ON positions (strategy_id);
CREATE INDEX idx_positions_status ON positions (status);
