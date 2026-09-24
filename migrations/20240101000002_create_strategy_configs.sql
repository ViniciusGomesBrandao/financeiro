-- Uma linha por instância de estratégia configurada (um `StrategyId` + seus
-- parâmetros), não por *kind* de estratégia. `strategy_kind` identifica qual
-- implementação em Rust esta configuração controla (por exemplo
-- "ema_crossover").
CREATE TABLE strategy_configs (
    id                          TEXT PRIMARY KEY,
    strategy_kind               TEXT NOT NULL,
    params                      JSONB NOT NULL,
    supported_asset_classes     TEXT[] NOT NULL,
    required_market_data        TEXT[] NOT NULL,
    enabled                     BOOLEAN NOT NULL DEFAULT true,
    created_at                  TIMESTAMPTZ NOT NULL DEFAULT now()
);
