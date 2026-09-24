-- Estado da carteira em um instante, para reconstruir a curva de equity.
-- Escrita periodicamente pelo loop da aplicação, não a cada tick.
CREATE TABLE portfolio_snapshots (
    id                      BIGSERIAL PRIMARY KEY,
    "timestamp"             TIMESTAMPTZ NOT NULL,
    cash                    NUMERIC NOT NULL,
    equity                  NUMERIC NOT NULL,
    realized_pnl            NUMERIC NOT NULL,
    unrealized_pnl          NUMERIC NOT NULL,
    open_positions_count    INT NOT NULL,
    exposure_ratio          NUMERIC NOT NULL,
    return_pct              NUMERIC NOT NULL
);

CREATE INDEX idx_portfolio_snapshots_timestamp ON portfolio_snapshots ("timestamp");
