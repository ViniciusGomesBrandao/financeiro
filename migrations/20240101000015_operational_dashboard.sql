-- Dashboard operacional: robôs configuráveis pelo usuário e telemetria do
-- Strategy Judge (estado ativo, avaliações e trocas).

CREATE TABLE operational_robots (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    symbol TEXT NOT NULL,
    timeframe TEXT NOT NULL,
    candidate_kinds TEXT[] NOT NULL,
    paper_capital DECIMAL NOT NULL,
    status TEXT NOT NULL DEFAULT 'stopped',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT operational_robots_status_check CHECK (status IN ('running', 'stopped'))
);

CREATE INDEX idx_operational_robots_symbol ON operational_robots (symbol);
CREATE INDEX idx_operational_robots_status ON operational_robots (status);

CREATE TABLE active_strategy_state (
    instrument_id UUID PRIMARY KEY,
    robot_id TEXT REFERENCES operational_robots (id) ON DELETE SET NULL,
    selected_strategy_id TEXT,
    evaluated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE judge_evaluations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instrument_id UUID NOT NULL,
    robot_id TEXT REFERENCES operational_robots (id) ON DELETE SET NULL,
    evaluated_at TIMESTAMPTZ NOT NULL,
    selected_strategy_id TEXT,
    decisions_json JSONB NOT NULL
);

CREATE INDEX idx_judge_evaluations_instrument_time
    ON judge_evaluations (instrument_id, evaluated_at DESC);

CREATE TABLE strategy_switches (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instrument_id UUID NOT NULL,
    robot_id TEXT REFERENCES operational_robots (id) ON DELETE SET NULL,
    previous_strategy_id TEXT,
    new_strategy_id TEXT,
    reason_json JSONB NOT NULL,
    switched_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_strategy_switches_instrument_time
    ON strategy_switches (instrument_id, switched_at DESC);
