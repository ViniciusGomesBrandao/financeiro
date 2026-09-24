# Estado do projeto — resumo

Documento de visão rápida: **o que o software é**, **o que já está pronto** e **o que ainda falta**. Para decisões arquiteturais detalhadas, veja [architecture.md](./architecture.md). Para o guia completo de desenvolvimento, veja o [README](../README.md) na raiz.

Atualizado em: 2026-09-12.

---

## 1. Ideia do software

`quant-engine` é um **laboratório de trading quantitativo** focado em:

1. Observar mercados reais (hoje: cripto Binance Spot).
2. Rodar **várias estratégias** sobre os mesmos dados.
3. Deixar um **Strategy Judge** decidir qual estratégia é economicamente mais adequada naquele momento.
4. Operar em **paper trading** (dinheiro fictício) com risco, execução simulada, P&L e persistência.
5. Permitir que uma pessoa acompanhe e controle isso por um **dashboard**, sem depender só do terminal.

### Hipótese de produto

> Várias estratégias observam o mesmo mercado → o Judge avalia viabilidade econômica → o robô opera a escolhida → o Judge continua avaliando → se outra ficar melhor, o robô pode trocar.

Objetivo financeiro: descobrir **edges estatísticos** (padrões que sobrevivem a fees/slippage realistas). **Não há promessa de lucro.** Paper trading existe para invalidar hipóteses *antes* de dinheiro real.

### Princípios que não mudam

- Separação `Signal` → `Risk` → `Order` → `Position` (estratégia não manda ordem direto).
- Um processo (`quant-engine`), não microsserviços.
- Core agnóstico de cripto (ações são extensão futura).
- Sem execução real até decisão explícita de segurança.
- Dashboard **consome** o backend; não recalcula Judge/estratégia no frontend.

---

## 2. O que está pronto hoje

### 2.1 Motor de trading (paper)

| Peça | Status |
|------|--------|
| Dados ao vivo Binance Spot (REST + WebSocket) | Pronto |
| Instrumentos BTC/USDT, ETH/USDT, SOL/USDT (configurável) | Pronto |
| Pipeline: candle → estratégias → risco → paper broker → portfolio → Postgres | Pronto |
| Risk engine (tamanho, exposição, stops, perda diária) | Pronto |
| Paper broker com fees + slippage | Pronto |
| Portfolio com caixa, posições, P&L; chave `(instrumento, strategy_id)` | Pronto |
| Restart do motor restaura posições/caixa do Postgres | Pronto |

### 2.2 Strategy Library (6 estratégias)

| Kind | Tipo |
|------|------|
| `mean_reversion` | Baseline |
| `momentum` | Baseline |
| `ema_crossover` | Baseline |
| `statistical_mean_reversion` | Quantitativa (features) |
| `quant_momentum` | Quantitativa (features) |
| `volatility_breakout` | Quantitativa (features) |

- Catálogo único (`strategies::catalog`).
- Múltiplas **instâncias** do mesmo algoritmo (`StrategyInstance`, ids como `{robot}::{kind}`).
- Compatibilidade por classe de ativo / market data exigido.

### 2.3 Strategy Judge + troca automática

| Capacidade | Status |
|------------|--------|
| Avaliar candidatas (win rate, profit factor, expectancy, drawdown, amostra) | Pronto |
| Classificação de **regime de mercado** (trending / ranging / vol. expansion / uncertain) | Pronto (Fase 2) |
| Seleção contextual regime × kind + fallback econômico | Pronto (Fase 2) |
| Estados `Active` / `Degraded` / `Disabled` | Pronto |
| Histerese (`min_confirmations`) anti-troca por ruído (estado + seleção por regime) | Pronto |
| Seleção da ativa **por robô** (mesmo símbolo/TF isolados) | Pronto (Fase A) |
| Bloquear Long de estratégias não selecionadas | Pronto |
| Fechar posição da estratégia destituída | Pronto |
| Persistir avaliações, estado ativo e trocas | Pronto |

### 2.4 Feature engine e backtest

| Peça | Status |
|------|--------|
| Feature engine (indicadores OHLCV sem look-ahead) | Pronto |
| Backtest runner no mesmo caminho Signal→Risk→Broker | Pronto |
| Dados históricos / Parquet (`historical-data`) | Parcial (infra existe; uso operacional ainda limitado) |
| Experiments / microstructure | Crates presentes; uso avançado ainda aberto |

### 2.5 Dashboard web

| Capacidade | Status |
|------------|--------|
| Observabilidade: overview, preços, posições, trades, performance, timeline | Pronto |
| Aba **Operacional**: criar robô, símbolo, timeframe, candidatas, capital fictício | Pronto |
| Iniciar / parar robô via API | Pronto (ver limitações abaixo) |
| Ver estratégia ativa, humor do Judge, motivo | Pronto |
| Comparação de candidatas, métricas, histórico, trades, curva de P&L acumulado | Pronto |
| Identidade visual + Animate UI + UX “frase → microcomponente → detalhe” | Pronto |
| Poll ~5s | Pronto (não é WebSocket) |

### 2.6 Persistência operacional

Tabelas relevantes além do núcleo clássico:

- `operational_robots`
- `active_strategy_state` (PK `robot_id`)
- `robot_portfolio_snapshots` (caixa/equity por robô)
- `judge_evaluations`
- `strategy_switches`
- `strategy_configs` (inclui `enabled` para start/stop)

### 2.7 Como rodar (resumo)

```bash
docker compose up -d
cargo run -p app          # quant-engine (paper)
cargo run -p web          # UI em http://127.0.0.1:58080
# frontend (se alterar UI):
cd crates/web/frontend && npm run build
```

---

## 3. Limitações conhecidas do que já existe

Estas não são “bugs escondidos”; são decisões / débitos conscientes:

1. **Start de robô novo** — precisa **reiniciar** o `quant-engine` para carregar instâncias novas. **Parar** já bloqueia novos Long sem restart (`enabled = false`).
2. **Sem venda a descoberto** (spot). Ordens `Sell` no feed são **fechamento** de compra, não short.
3. **Sem execução real** (e continua proibida até decisão explícita).
4. **Paper broker** só `Market` orders (sem limit/parcial sofisticado).
5. **Judge de regime** — classificação determinística simples (trending / ranging / volatility expansion / uncertain) com limiares alinhados às estratégias existentes; não é ML nem probabilidade calibrada.
6. **Compartilhamento de capital entre robots** — fora de escopo; cada robô tem ledger próprio.

### Fase A concluída (unidade operacional Robot)

- Timeframe do robô alimenta stream/warm-up/pipeline (não só UI/DB).
- Capital paper isolado por robô (`paper_capital` → `PortfolioManager` + `robot_portfolio_snapshots`).
- Judge / estratégia ativa keyed por `robot_id`.
- Equity API/dashboard = cash + mark das posições daquele robô; P&L de performance filtrado por instâncias do robô.
- Posições de `strategy_id` de teste continuam excluídas do restore ao vivo.

---

## 4. O que falta implementar

Organizado por prioridade prática (não é cronograma fechado).

### 4.1 Fechar o ciclo “robô de verdade” (próximo salto de produto)

| Item | Por quê |
|------|---------|
| Hot-reload / orquestração do registry | Criar/iniciar robô sem reiniciar o processo |
| Refinar limiares de regime com evidência empírica | Hoje reutilizam defaults das estratégias |
| Sinal claro “config já aplicada pelo engine” | Substituir o aviso genérico de restart |

### 4.2 Dashboard / UX

| Item | Por quê |
|------|---------|
| Editar / apagar robô depois de criado | Hoje só create + status |
| Equity / métricas de conta com narrativa por robô mais rica | Sharpe etc. ainda limitados; equity isolada já existe (Fase A) |
| Push em tempo real (SSE/WebSocket) | Hoje é poll HTTP |
| Retenção / limpeza de `judge_evaluations` | Cresce a cada candle |

### 4.3 Pesquisa e qualidade estatística

| Item | Por quê |
|------|---------|
| Histórico em massa (Parquet) + walk-forward | Backtest sério em escala |
| Métricas de série temporal (Sharpe, Sortino, correlação) | Já listadas como gap no README |
| Feature store / rastreio de hipóteses | Roadmap de pesquisa |
| Mais execução paper (limit, parciais, book) | Fidelidade da simulação |

### 4.4 Expansão de mercado / produção

| Item | Por quê |
|------|---------|
| Execução real limitada em cripto | Só após decisão explícita de segurança |
| Adaptadores de ações (B3, NYSE, …) | Calendário, sessões, dados |
| Observabilidade/ops de produção | Alertas, multi-processo se algum dia precisar |

---

## 5. Mapa mental rápido

```text
Binance (ao vivo, stream por timeframe)
    → market-data
    → Robot (symbol + TF + capital + portfolio)
         → strategies (candidatas do robô) + features
         → strategy-judge (ativa por robô)
         → risk
         → execution (paper)
         → portfolio isolado
    → persistence (Postgres)
    → web (observabilidade + operacional)
```

**Pronto o suficiente para:** experimento real de paper trading com **dois robôs isolados** (ex.: BTC 15m $1k e ETH 15m $1k) sem compartilhar capital, posições, equity, Judge ou estratégia ativa.

**Ainda não pronto para:** dinheiro real, Judge de regime, ou operação sem reinício do motor ao criar robôs novos.

---

## 6. Referências

- [README.md](../README.md) — fonte de verdade completa
- [architecture.md](./architecture.md) — ADRs
- Crates principais: `app`, `strategies`, `strategy-judge`, `risk`, `portfolio`, `execution`, `persistence`, `web`, `analytics`, `features`, `backtest`
