-- Último preço conhecido por instrumento, atualizado a cada candle
-- fechada processada pelo pipeline live. Existe exclusivamente para a UI
-- de observabilidade conseguir mostrar "preço atual" sem manter estado em
-- memória compartilhado entre o processo do pipeline e o processo web —
-- não é um histórico de ticks (isso continua fora de escopo, ver README).
CREATE TABLE latest_prices (
    instrument_id   UUID PRIMARY KEY REFERENCES instruments (id),
    price           NUMERIC NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL
);
