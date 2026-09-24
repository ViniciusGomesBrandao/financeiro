-- Eventos individuais de fill contra uma order. Hoje o `PaperBroker` produz
-- exatamente um fill por market order, mas o schema não assume isso.
CREATE TABLE fills (
    id                  UUID PRIMARY KEY,
    order_id            UUID NOT NULL REFERENCES orders (id),
    price               NUMERIC NOT NULL,
    quantity            NUMERIC NOT NULL,
    fee                 NUMERIC NOT NULL,
    liquidity           TEXT NOT NULL,
    executed_at         TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_fills_order_id ON fills (order_id);
