-- Ledger append-only de trades completos de ida e volta, uma linha por
-- posição fechada. Mantida separada de `positions` (que também guarda linhas
-- abertas e é consultada para o estado vivo) para que analytics possa varrer
-- uma tabela estritamente de trades fechados sem filtrar por status. De
-- propósito não armazena histórico bruto de ticks/ohlcv do mercado — ver o
-- README raiz, "Persistência", para o motivo de isso ficar para um futuro
-- data lake baseado em Parquet.
CREATE TABLE trades (
    id                  UUID PRIMARY KEY,
    position_id         UUID NOT NULL REFERENCES positions (id),
    instrument_id       UUID NOT NULL REFERENCES instruments (id),
    strategy_id         TEXT NOT NULL REFERENCES strategy_configs (id),
    side                TEXT NOT NULL,
    quantity            NUMERIC NOT NULL,
    entry_price         NUMERIC NOT NULL,
    exit_price          NUMERIC NOT NULL,
    opened_at           TIMESTAMPTZ NOT NULL,
    closed_at           TIMESTAMPTZ NOT NULL,
    pnl_gross           NUMERIC NOT NULL,
    pnl_net             NUMERIC NOT NULL,
    fees_paid           NUMERIC NOT NULL
);

CREATE INDEX idx_trades_strategy_id ON trades (strategy_id);
CREATE INDEX idx_trades_instrument_id ON trades (instrument_id);
CREATE INDEX idx_trades_closed_at ON trades (closed_at);
