-- Fase A: isolamento operacional por robô.
-- Estado ativo do Judge passa a ser por robot_id (não só por instrumento).
-- Snapshots de portfólio por robô para capital/equity isolados.
--
-- Sem FK para operational_robots: o modo legado (robots sintéticos
-- `legacy-{symbol}`) e testes de integração usam robot_id que não
-- existem na tabela operacional; o vínculo lógico continua sendo a
-- string robot_id.

DROP TABLE IF EXISTS active_strategy_state;

CREATE TABLE active_strategy_state (
    robot_id TEXT PRIMARY KEY,
    instrument_id UUID NOT NULL,
    selected_strategy_id TEXT,
    evaluated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_active_strategy_state_instrument
    ON active_strategy_state (instrument_id);

CREATE TABLE IF NOT EXISTS robot_portfolio_snapshots (
    id BIGSERIAL PRIMARY KEY,
    robot_id TEXT NOT NULL,
    "timestamp" TIMESTAMPTZ NOT NULL,
    cash DECIMAL NOT NULL,
    equity DECIMAL NOT NULL,
    realized_pnl DECIMAL NOT NULL,
    unrealized_pnl DECIMAL NOT NULL,
    open_positions_count INTEGER NOT NULL,
    exposure_ratio DECIMAL NOT NULL,
    return_pct DECIMAL NOT NULL,
    realized_pnl_today DECIMAL NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_robot_portfolio_snapshots_robot_time
    ON robot_portfolio_snapshots (robot_id, "timestamp" DESC);
