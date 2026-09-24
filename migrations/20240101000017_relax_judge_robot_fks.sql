-- Robôs sintéticos do modo legado (`legacy-{symbol}`) e testes usam
-- robot_id que não existe em operational_robots. A telemetria do Judge
-- precisa aceitar esses ids sem FK rígida (mesmo racional da migration 16).

ALTER TABLE judge_evaluations
    DROP CONSTRAINT IF EXISTS judge_evaluations_robot_id_fkey;

ALTER TABLE strategy_switches
    DROP CONSTRAINT IF EXISTS strategy_switches_robot_id_fkey;

CREATE INDEX IF NOT EXISTS idx_judge_evaluations_robot_time
    ON judge_evaluations (robot_id, evaluated_at DESC);

CREATE INDEX IF NOT EXISTS idx_strategy_switches_robot_time
    ON strategy_switches (robot_id, switched_at DESC);
