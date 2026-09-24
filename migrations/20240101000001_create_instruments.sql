-- Instrumentos negociáveis, independentes do formato de transporte de
-- qualquer exchange específica. Populada na inicialização a partir dos
-- metadados do provider de market data (por exemplo o exchangeInfo da
-- Binance), não mantida à mão.
CREATE TABLE instruments (
    id                  UUID PRIMARY KEY,
    symbol              TEXT NOT NULL,
    base_asset          TEXT NOT NULL,
    quote_asset         TEXT NOT NULL,
    asset_class         TEXT NOT NULL,
    exchange            TEXT NOT NULL,
    market_type         TEXT NOT NULL,
    tick_size           NUMERIC NOT NULL,
    lot_size            NUMERIC NOT NULL,
    min_quantity        NUMERIC NOT NULL,
    min_notional        NUMERIC NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (symbol, exchange, market_type)
);
