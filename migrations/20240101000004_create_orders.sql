-- Intenções de order e seu ciclo de vida. `signal_id` aceita nulo porque
-- uma saída forçada pelo risk engine (stop loss / take profit / limite de
-- perda diária) pode originar uma order sem nenhum sinal de estratégia por
-- trás.
CREATE TABLE orders (
    id                  UUID PRIMARY KEY,
    instrument_id       UUID NOT NULL REFERENCES instruments (id),
    strategy_id         TEXT NOT NULL REFERENCES strategy_configs (id),
    signal_id           UUID REFERENCES signals (id),
    side                TEXT NOT NULL,
    order_type          TEXT NOT NULL,
    quantity            NUMERIC NOT NULL,
    limit_price         NUMERIC,
    status              TEXT NOT NULL,
    filled_quantity     NUMERIC NOT NULL,
    average_fill_price  NUMERIC,
    created_at          TIMESTAMPTZ NOT NULL,
    updated_at          TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_orders_instrument_id ON orders (instrument_id);
CREATE INDEX idx_orders_strategy_id ON orders (strategy_id);
