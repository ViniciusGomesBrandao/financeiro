-- Consolidados periódicos de desempenho por estratégia, calculados pelo
-- crate `analytics` e persistidos pelo loop da aplicação. Uma linha por
-- cálculo (strategy_id, as_of) — não é atualizada no lugar, então o
-- histórico de como as métricas evoluíram é preservado.
CREATE TABLE strategy_performance (
    id                  BIGSERIAL PRIMARY KEY,
    strategy_id         TEXT NOT NULL REFERENCES strategy_configs (id),
    as_of               TIMESTAMPTZ NOT NULL,
    total_trades        INT NOT NULL,
    winners             INT NOT NULL,
    losers              INT NOT NULL,
    win_rate            NUMERIC NOT NULL,
    gross_pnl           NUMERIC NOT NULL,
    net_pnl             NUMERIC NOT NULL,
    average_win         NUMERIC NOT NULL,
    average_loss        NUMERIC NOT NULL,
    profit_factor       NUMERIC,
    max_drawdown        NUMERIC NOT NULL
);

CREATE INDEX idx_strategy_performance_strategy_id ON strategy_performance (strategy_id);
CREATE INDEX idx_strategy_performance_as_of ON strategy_performance (as_of);
