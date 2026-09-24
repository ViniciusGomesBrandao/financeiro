# quant-engine

Um laboratório de trading quantitativo pequeno, real e extensível. Hoje
somente cripto, somente paper trading, arquitetado de modo que ações
possam ser adicionadas depois sem reconstruir o core.

Este README é a principal fonte de verdade do projeto — para humanos **e**
para assistentes de IA de programação (Claude, Cursor, ChatGPT, ...) que
assumirem trabalho aqui. Leia-o antes de fazer mudanças arquiteturais. Veja
especialmente o **[Guia de desenvolvimento com IA](#19-guia-de-desenvolvimento-com-ia)**
no final.

---

## Índice

1. [Visão do produto](#1-visão-do-produto)
2. [Objetivo financeiro](#2-objetivo-financeiro)
3. [Estado atual](#3-estado-atual)
4. [Arquitetura](#4-arquitetura)
5. [Fluxo de dados](#5-fluxo-de-dados)
6. [Princípios arquiteturais](#6-princípios-arquiteturais)
7. [Compatibilidade de estratégias](#7-compatibilidade-de-estratégias)
    - [7b. Estratégias quantitativas baseadas em features](#7b-estratégias-quantitativas-baseadas-em-features)
    - [7c. Strategy Library — catálogo e descoberta](#7c-strategy-library--catálogo-e-descoberta)
        - [7c.1. `strategies::instance` — múltiplas instâncias por dado (Fase 1.5)](#7c1-strategiesinstance--múltiplas-instâncias-por-dado-fase-15)
8. [Como criar uma nova estratégia](#8-como-criar-uma-nova-estratégia)
9. [Como adicionar uma nova exchange de cripto](#9-como-adicionar-uma-nova-exchange-de-cripto)
10. [Como adicionar ações no futuro](#10-como-adicionar-ações-no-futuro)
11. [Modelo de domínio](#11-modelo-de-domínio)
12. [Banco de dados](#12-banco-de-dados)
13. [Configuração](#13-configuração)
14. [Como executar](#14-como-executar)
    - [14b. UI web de observabilidade](#14b-ui-web-de-observabilidade)
15. [Como testar](#15-como-testar)
16. [Limitações atuais](#16-limitações-atuais)
17. [Segurança](#17-segurança)
18. [Roadmap](#18-roadmap)
19. [Guia de desenvolvimento com IA](#19-guia-de-desenvolvimento-com-ia)

---

## 1. Visão do produto

`quant-engine` é um laboratório de pesquisa e trading quantitativo. Ele foi
construído para:

- ingerir dados reais de mercado;
- analisar múltiplos instrumentos simultaneamente;
- rodar estratégias quantitativas sobre esses dados;
- transformar a saída das estratégias em sinais;
- aplicar regras de risco a esses sinais;
- simular a execução de ordens (paper trading);
- registrar cada ordem, fill e posição;
- calcular P&L e métricas de desempenho;
- depois: rodar backtests rigorosos sobre dados históricos;
- depois: (opcionalmente, após uma decisão futura explícita) executar
  trades reais;
- depois: ir além de cripto, alcançando mercados de ações (B3, NYSE,
  NASDAQ, ...).

Ele começa exclusivamente com criptomoedas, mas nada no core (domínio,
motor de estratégias, risco, portfólio, paper trading, persistência,
analytics, backtesting) assume cripto especificamente. Veja
[§10](#10-como-adicionar-ações-no-futuro).

## 2. Objetivo financeiro

Este projeto existe para ajudar a identificar **edges estatísticos** —
padrões repetíveis que se possa demonstrar empiricamente que uma estratégia
captura melhor do que o acaso, líquido de fees e slippage realistas.

**Não há promessa de retorno.** Nenhuma estratégia entregue aqui é
presumida lucrativa. O paper trading existe especificamente para validar
uma hipótese *antes* que qualquer dinheiro real venha a ser envolvido — e
habilitar dinheiro real é uma decisão futura deliberada, não um default
(veja [§17](#17-segurança)).

Trate todo resultado de backtest e de paper trading como uma amostra
estatística, não como uma garantia. Uma estratégia que vai bem nos dados
entregues é uma hipótese inicial para testes adicionais, não uma conclusão.

## 3. Estado atual

**Existe hoje:**

- Um workspace Cargo de 12 crates (abaixo), todos compilando limpos sob
  `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D
  warnings` e `cargo test --workspace`.
- Dados públicos de mercado ao vivo do Binance Spot (klines via REST +
  streams WebSocket combinados de kline/trade, com reconexão automática)
  para BTC/USDT, ETH/USDT, SOL/USDT (configurável).
- Três estratégias genéricas baseline (EMA crossover, momentum, mean
  reversion) com racional, parâmetros e limitações documentados, mais três
  estratégias quantitativas (Statistical Mean Reversion, Quant Momentum,
  Volatility Breakout) consumindo `features::FeatureSnapshot` via
  `FeatureStrategy` em vez de recalcular indicadores — veja
  [§7b](#7b-estratégias-quantitativas-baseadas-em-features).
- Um mecanismo explícito de compatibilidade de estratégias
  (`StrategyRegistry`) que se recusa a registrar uma estratégia contra um
  instrumento que ela não suporta.
- Um motor de risco que impõe tamanho de posição, exposição, número de
  posições, stop loss, take profit e limites de perda diária.
- Um paper broker totalmente simulado, com modelagem de fee + slippage.
- Um gerenciador de portfólio acompanhando caixa, posições e P&L.
- Persistência em PostgreSQL (via SQLx, queries verificadas em runtime)
  para instrumentos, configs de estratégia, sinais, ordens, fills,
  posições, trades, snapshots de portfólio e rollups de desempenho por
  estratégia.
- Analytics de desempenho no nível de trade (P&L, taxa de acerto, profit
  factor, max drawdown, ...).
- Um Feature Engine (`features`, veja o crate abaixo) calculando 13
  famílias de indicadores sobre OHLCV — retornos multi-horizonte, SMA/EMA,
  desvio padrão, volatilidade realizada, volatilidade EWMA, z-score,
  Bollinger Bands, ATR, RSI, momentum/ROC, regressão linear rolling
  (slope + R²), autocorrelação, volume relativo — sem look-ahead bias por
  construção, e integrado ao backtest (`BacktestReport::feature_snapshots`).
  As estratégias genéricas atuais ainda calculam seus próprios indicadores
  internamente e não consomem isto ainda (ver a nota de escopo no crate).
- Um runner de backtest que reproduz candles históricos pelo *mesmo*
  pipeline de estratégia/risco/broker/portfólio que a aplicação ao vivo usa,
  e que também alimenta um `FeatureEngine` por instrumento durante a
  reprodução.
- Um binário executável (`quant-engine`) conectando tudo isso.
- Uma UI web local e somente leitura de observabilidade (`quant-engine-web`,
  veja [§14b](#14b-ui-web-de-observabilidade)) mostrando saldo/equity/P&L,
  preços monitorados, posições abertas, histórico de trades, desempenho por
  estratégia e uma timeline legível de sinal → motivo → decisão de risco →
  execução → fechamento/P&L — sem qualquer capacidade de execução.

**Ainda não existe (veja [§16](#16-limitações-atuais) e
[§18](#18-roadmap)):**

- Execução com dinheiro real de qualquer tipo.
- Dados de mercado, calendários ou brokers de ações.
- Simulação de ordens limit / fills maker (apenas fills market/taker são
  simulados).
- Analytics de Sharpe/Sortino/expectância/volatilidade/correlação.
- Um armazenamento de dados históricos em massa (Parquet ou outro) — apenas
  dados operacionais recentes vivem no Postgres.
- Estratégias de derivativos cripto (funding rate, open interest) — os
  feeds de dados de que precisam ainda não são consumidos.

## 4. Arquitetura

Um único workspace Cargo, **não** uma frota de microsserviços — dois
pequenos binários (`crates/app`, o pipeline de trading, e `crates/web`, uma
UI de observabilidade somente leitura), doze crates no total, nenhuma outra
peça móvel.

```text
quant-engine/
├── Cargo.toml                  manifesto do workspace + versões compartilhadas de dependências
├── migrations/                 migrations SQL (flat, aplicadas por `persistence`)
├── config/                     reservado para config estruturada futura (veja config/README.md)
├── docker-compose.yml          apenas o Postgres local
├── .env.example
│
└── crates/
    ├── domain/          tipos puros + invariantes, zero dependências de infra
    ├── market-data/     trait MarketDataProvider + adaptador Binance Spot
    ├── strategies/      trait Strategy, motor de compatibilidade, estratégias generic/crypto/equities
    ├── risk/            RiskEngine: Signal -> OrderRequest | Rejected(reason)
    ├── portfolio/       PortfolioManager: caixa, posições, P&L — o único dono desse estado
    ├── execution/       trait Broker + PaperBroker (totalmente simulado, ciente de fee+slippage)
    ├── persistence/     PostgreSQL via SQLx, depende apenas de `domain`
    ├── analytics/       métricas de desempenho no nível de trade, depende apenas de `domain`
    ├── features/        Feature Engine: indicadores OHLCV, sem look-ahead, independente de estratégias
    ├── backtest/        reproduz MarketEvents históricos pelo pipeline ao vivo
    ├── app/              binário: config, wiring, o event loop ao vivo
    └── web/               binário: UI HTTP de observabilidade somente leitura (veja §14b)
```

### Crate a crate

**`domain`** — Tipos agnósticos a exchange e a broker, compartilhados por
todos os outros crates: `Instrument`, `AssetClass`, `Candle`,
`MarketTrade`, `OrderBookSnapshot`, `Signal`, `OrderRequest`/`Order`,
`Position`, `PortfolioSnapshot`, `Timeframe`, `Price`/`Quantity`/`Money`
(todos baseados em `rust_decimal`), `TradingCalendar`. Sem `tokio`, sem
`sqlx`, sem cliente HTTP — este crate é puramente dados e invariantes. Veja
[§11](#11-modelo-de-domínio).

**`market-data`** — Trait `MarketDataProvider` (capacidades + buscar
candles recentes + buscar metadados de instrumento + stream ao vivo) e uma
implementação, `binance::BinanceMarketData`, consumindo os endpoints REST e
WebSocket **públicos** do Binance Spot (sem API key, sem capacidade de envio
de ordens). Todos os formatos JSON da Binance são privados a
`binance::dto`/`binance::normalize` — nada fora deste crate jamais os vê.

**`strategies`** — O trait `Strategy`, `StrategyRequirements` (o contrato
declarado de dados/classe de ativo) e `StrategyRegistry` (o ponto de
imposição da compatibilidade — veja
[§7](#7-compatibilidade-de-estratégias)). Também `feature_strategy`: o
trait `FeatureStrategy` (id, requirements, `feature_config`,
`on_features(&FeatureSnapshot)` — quatro métodos, nenhum acesso a
`MarketEvent` bruto) e `FeatureStrategyAdapter<S: FeatureStrategy>`, que
adapta qualquer `FeatureStrategy` para `Strategy`, mantendo um
`features::FeatureEngine` por instrumento (mesma convenção de estado
por-instrumento que os indicadores internos das baselines já usam) e
alimentando-o um candle fechado por vez — a mesma garantia estrutural de
não-look-ahead do `FeatureEngine`, estendida a qualquer estratégia que
implemente `FeatureStrategy`. As estratégias são organizadas como:
- `generic/` — válidas para múltiplas classes de ativos porque o cálculo é
  genuinamente agnóstico à classe de ativo. Duas gerações lado a lado:
  - **Baselines** (`ema_crossover`, `momentum`, `mean_reversion`) —
    implementam `Strategy` diretamente, calculando seus próprios
    indicadores. Mantidas sem alteração, como referência de comparação.
  - **Quantitativas** (`statistical_mean_reversion`, `quant_momentum`,
    `volatility_breakout`) — implementam `FeatureStrategy`, consumindo
    `features::FeatureSnapshot`; cada uma combina 2-3 features por filtro
    conjuntivo (nunca peso arbitrário) — ver a lógica resumida de cada uma
    em [§7b](#7b-estratégias-quantitativas-baseadas-em-features).
- `crypto/` — estratégias específicas de cripto (nenhuma implementada
  ainda; placeholder documentado para uma estratégia de funding rate).
- `equities/` — estratégias específicas de ações (nenhuma implementada
  ainda; placeholder documentado para uma estratégia sensível à sessão).

**`risk`** — `RiskEngine::evaluate(signal, instrument, price,
&PortfolioState) -> RiskDecision` (`Approved(OrderRequest)` ou
`Rejected(RejectionReason)`), além de `check_exit`/`build_exit_order` para
monitoramento de stop loss/take profit. `PortfolioState` é uma struct de
snapshot somente leitura (não um trait, não um estado próprio), de modo que
`risk` nunca depende de `portfolio` e nunca muta nada. Impõe um invariante
de spot: um sinal `Short` só é acionável como saída de uma posição comprada
existente; sem posição aberta ele é rejeitado
(`RejectionReason::ShortSellingNotSupported`), nunca convertido em uma
venda a descoberto nua. `execution::PaperBroker` recusa o mesmo caso de
forma independente, como defesa em profundidade — veja o ADR-8 em
`docs/architecture.md`. Também arredonda a quantidade da ordem para baixo
até `instrument.lot_size` antes de qualquer outra checagem (via
`domain::Instrument::round_quantity_down_to_lot`), de modo que todo número
downstream reflita o que poderia de fato ser executado — veja ADR-10.

**`portfolio`** — `PortfolioManager`: o único dono do caixa, das posições
abertas/fechadas, do P&L realizado/não realizado, da exposição e do
retorno. Tanto `risk` (snapshot somente leitura) quanto `execution` (via
`open_position`/`close_position`) o consomem; nenhum duplica sua
escrituração. `Position` carrega `fees_paid`/`spread_paid`/`slippage_paid`
para que o custo de um trade fechado seja atribuível por componente, e não
apenas compensado dentro de `realized_pnl_net` — veja ADR-11.

**`execution`** — O trait `Broker` e o `PaperBroker`, sua única
implementação. `PaperBroker` é totalmente simulado: fees maker/taker,
spread e slippage configuráveis (modelados e registrados separadamente —
veja ADR-11), sem conectividade real com exchange, sem dinheiro real,
jamais (veja [§17](#17-segurança)). O preço de execução é quantizado para
`instrument.tick_size` e a quantidade para `instrument.lot_size`
(`domain::Instrument::round_price_to_tick`/`round_quantity_down_to_lot`),
espelhando o mesmo arredondamento que `risk` já aplica quando dimensiona
uma ordem — veja ADR-10.

**`persistence`** — Repositórios SQLx (Postgres), um módulo por tabela,
dependendo apenas de `domain`. Toda query é verificada em runtime
(`sqlx::query`/`query_as`), não pela família de macros de tempo de
compilação `query!` — veja os docs de módulo do crate para o porquê.
`instruments::upsert` trata o Postgres como autoridade sobre
`InstrumentId`: ele retorna qual id é agora autoritativo para a chave
natural `(symbol, exchange, market_type)`, de modo que os `InstrumentId`s
permaneçam estáveis entre reinícios do processo — veja ADR-9. Também guarda
`latest_prices` (preço de marcação atual por instrumento) e
`risk_decisions` (toda decisão do motor de risco, aprovada ou rejeitada,
com seu motivo) — ambas escritas pelo pipeline de `app` puramente para que
`web` tenha algo durável a ler; veja ADR-12.

**`analytics`** — Funções puras computando `PerformanceReport` (P&L
bruto/líquido, taxa de acerto, ganho/perda médios, profit factor, uma
aproximação de max drawdown baseada na sequência de trades) a partir de uma
slice de `domain::Position`. Depende apenas de `domain`. Reutilizado tal
como está tanto por `app` (resumo no shutdown) quanto por `web` (desempenho
na visão geral/por estratégia) — a matemática das métricas existe em
exatamente um lugar.

**`features`** — Feature Engine reutilizável, independente de qualquer
estratégia: `FeatureEngine::new(instrument_id, FeatureConfig)` +
`update(&Candle) -> Option<FeatureSnapshot>`, alimentado um candle fechado
por vez, em ordem cronológica — o mesmo padrão de despacho que
`strategies::Strategy::on_event` já usa, e é isso que torna a ausência de
look-ahead bias uma garantia estrutural (o motor nunca vê um candle antes
da hora), não uma disciplina que cada chamador precisa lembrar de manter.
Calcula 13 famílias de indicadores sobre OHLCV — retornos multi-horizonte,
SMA, EMA, desvio padrão, volatilidade realizada, volatilidade EWMA,
z-score, Bollinger Bands, ATR, RSI, momentum/ROC, regressão linear rolling
(slope + R²), autocorrelação, volume relativo — cada um documentado
(significado, fórmula, limitações) no doc comment do próprio módulo em
`crates/features/src/ohlcv/`, com testes determinísticos por feature. Só
depende de `domain` (mesmo padrão de `analytics`); usa `f64`, não
`Decimal`, pela mesma razão que `strategies::generic::indicators::Ema` já
documenta (são leituras estatísticas, não dinheiro). Deliberadamente
**não** foi integrado às três estratégias genéricas existentes — elas
continuam calculando seus próprios indicadores internamente
(`strategies::generic::indicators`); migrá-las para consumir
`FeatureSnapshot` em vez de recalcular é um passo futuro, fora do escopo do
que introduziu este crate. Estruturado para um futuro módulo de
microestrutura (`domain::MarketTrade`/`domain::OrderBookSnapshot`) viver
como um módulo **irmão** de `ohlcv`, não uma extensão dele — ver o doc do
crate raiz para o porquê de não misturar as duas fontes num único
`FeatureSnapshot`.

**`backtest`** — `BacktestRunner::run(candles, &mut StrategyRegistry,
&RiskEngine, &mut dyn Broker, &mut PortfolioManager)`, reproduzindo candles
históricos literalmente pelo mesmo pipeline que a aplicação ao vivo aciona.
Também alimenta um `FeatureEngine` por instrumento durante a reprodução
(criado com `FeatureConfig::default()` via `BacktestRunner::new`, ou
customizado via `BacktestRunner::with_feature_config`) e devolve os
snapshots resultantes em `BacktestReport::feature_snapshots` — na mesma
ordem cronológica da reprodução, com a mesma garantia de não-look-ahead do
`FeatureEngine` (comprovada por um teste que reproduz o mesmo prefixo de
candles duas vezes, com e sem candles futuros adicionais, e verifica que o
snapshot já produzido não muda).

**`app`** — O binário do pipeline de trading (`quant-engine`). Carrega a
config do ambiente, conecta ao Postgres e roda as migrations, constrói o
provider da Binance, registra instrumentos e estratégias, faz o warmup do
estado das estratégias a partir de candles recentes, abre o stream ao vivo
e roda o event loop até Ctrl+C.

**`web`** — O binário da UI de observabilidade (`quant-engine-web`), veja
[§14b](#14b-ui-web-de-observabilidade). Conecta ao *mesmo* Postgres que
`app` e apenas lê dele (funções de consulta de `persistence`) — nunca toca
em `market-data`, `strategies`, `risk`, `execution` ou `portfolio`, e não
tem rota nem caminho de código que escreva qualquer coisa.

## 5. Fluxo de dados

```text
JSON da Binance
    -> market-data::binance (adaptador: parseia & normaliza)
    -> domain::Candle / domain::MarketTrade
    -> domain::MarketEvent
    -> strategies::StrategyRegistry::dispatch
    -> domain::Signal
    -> risk::RiskEngine::evaluate
    -> domain::OrderRequest        (apenas se Approved)
    -> execution::PaperBroker::submit_order
    -> portfolio::PortfolioManager (mutação de caixa/posição)
    -> persistence (signals, risk_decisions, orders, fills, positions,
                    trades, latest_prices, portfolio_snapshots)
    -> analytics::compute_performance (sob demanda / no shutdown / a partir do `web`)
    -> web::handlers (somente leitura) -> JSON -> navegador
```

Exatamente o mesmo trecho `MarketEvent -> Strategy -> Signal -> Risk ->
Broker -> Portfolio` é acionado por `crates/app/src/pipeline.rs` para dados
ao vivo e por `crates/backtest/src/runner.rs` para dados históricos —
nenhum reimplementa o wiring do outro. `web` fica inteiramente a jusante do
Postgres: ele nunca vê um `MarketEvent`, um `Signal` ou uma `RiskDecision`
diretamente, apenas as linhas que `app` já persistiu.

## 6. Princípios arquiteturais

1. **Abstrair mercados sem apagar suas diferenças.** `AssetClass`
   (`Crypto`, `CryptoDerivative`, `Equity`, `Future`, `Forex`) é o *único*
   conceito universal entre mercados no core. Tudo que é específico de um
   mercado (sessões de negociação, funding rates, peculiaridades de tick
   size) vive em código específico daquele mercado, não em uma interface
   compartilhada inchada.
2. **Estratégias genéricas apenas quando genuinamente genéricas.** Uma
   estratégia vai para `strategies/generic/` somente se seu cálculo for de
   fato agnóstico à classe de ativo. Uma estratégia que meramente
   *poderia* compilar contra outra classe de ativo, mas só faz sentido para
   uma, pertence a `crypto/` ou `equities/`.
3. **`Signal != Order`.** Um `Signal` é uma opinião sem quantidade, sem
   tipo de ordem e sem aprovação de risco. Só `risk::RiskEngine` pode
   transformá-lo em um `OrderRequest`.
4. **`Strategy != Risk`.** Uma implementação de `Strategy` não pode enviar
   ordens, tocar o banco de dados ou chamar uma exchange. Ela recebe dados
   de mercado e retorna sinais — ponto final.
5. **`MarketDataProvider != domain`.** Nenhum formato JSON da Binance (ou
   de uma exchange futura) cruza para fora de seu módulo adaptador. Tudo
   downstream fala apenas tipos de `domain`.
6. **`PaperBroker != Adaptador de Exchange`.** Ingestão de dados de mercado
   e execução de ordens são crates diferentes com responsabilidades
   diferentes, mesmo que o único broker de hoje (`PaperBroker`) nunca fale
   com uma exchange real.
7. **Um dono por pedaço de estado.** `portfolio::PortfolioManager` é o
   único dono do caixa/posições. `risk` lê um snapshot; `execution` muta
   através dos próprios métodos de `PortfolioManager`; ninguém mantém uma
   segunda cópia.
8. **Prefira a solução simples que preserva a evolução, em vez da
   sofisticada.** Quando uma escolha de design precisa optar entre
   "engenhosa e geral" e "pequena e correta, extensível depois", este
   projeto prefere a segunda — desde que ela não crie acoplamento
   estrutural que dificulte a extensão futura.

## 7. Compatibilidade de estratégias

Toda implementação de `Strategy` declara um `StrategyRequirements`:

```rust
pub struct StrategyRequirements {
    pub required_market_data: Vec<MarketDataKind>,   // e.g. [Ohlcv]
    pub supported_asset_classes: Vec<AssetClass>,     // e.g. [Crypto, Equity]
}
```

`strategies::check_compatible(strategy, instrument, available_market_data)`
é o único ponto de imposição:

- rejeita se `instrument.asset_class` não estiver em
  `supported_asset_classes`;
- rejeita se algum tipo em `required_market_data` não estiver presente em
  `available_market_data` (o que o(s) `MarketDataProvider`(s) configurado(s)
  de fato fornecem).

`StrategyRegistry::register(strategy, instruments, available_market_data)`
chama `check_compatible` para *cada* instrumento antes de aceitar
*qualquer* um deles — sem registro parcial, e nenhuma estratégia jamais
recebe um evento de um instrumento para o qual não foi explicitamente
aprovada. Isso é verificado uma vez, no momento do
registro/configuração (veja `crates/app/src/setup.rs`), nunca de forma
preguiçosa ou implícita no momento do sinal.

Exemplos concretos já codificados na base de código:

```text
EmaCrossoverStrategy / MomentumStrategy / MeanReversionStrategy
  requer:   [Ohlcv]
  suporta:  [Crypto, Equity]

(documentada, ainda não implementada) FundingRateStrategy
  requer:   [FundingRate]
  suporta:  [CryptoDerivative]

(documentada, ainda não implementada) MarketOpenStrategy
  requer:   [Ohlcv, TradingSession]
  suporta:  [Equity]
```

Veja os testes de `crates/strategies/src/registry.rs` para a imposição em
ação (`rejects_unsupported_asset_class`, `rejects_missing_market_data`,
`registry_refuses_to_register_incompatible_strategy`).

## 7b. Estratégias quantitativas baseadas em features

Três estratégias novas em `strategies::generic`, cada uma implementando
`FeatureStrategy` (não `Strategy` diretamente — ver `feature_strategy` no
crate `strategies`) e registrável no mesmo `StrategyRegistry` que as
baselines, envolvida em `FeatureStrategyAdapter`:

```rust
registry.register(
    Box::new(FeatureStrategyAdapter::new(QuantMomentumStrategy::new(id, params))),
    &instruments,
    &available_market_data,
)?;
```

Nenhuma combina features por peso/pontuação inventada — cada uma usa uma
única feature como gatilho primário (e a fonte da confiança do sinal) e as
demais como filtros binários de confirmação/regime, que só decidem *se* o
sinal existe, nunca *quanto*.

**Statistical Mean Reversion** (`StatisticalMeanReversionStrategy`) —
mesma premissa da baseline `MeanReversionStrategy` (preço muito distante
da média reverte), exigindo confirmação: `zscore` é o gatilho de entrada
*e* de saída (`|z| >= entry_z`); `rsi` precisa confirmar
sobrecompra/sobrevenda (`>= rsi_overbought` para short, `<= rsi_oversold`
para long); e `autocorrelation` dos retornos precisa estar *abaixo* de
`max_autocorrelation` — autocorrelação positiva forte é evidência de
tendência (o regime oposto ao que reversão à média assume), e nesse caso a
estratégia se abstém. Uma vez posicionada, para de avaliar entradas e
monitora só a reversão: emite `Flat` assim que `|z| <= exit_z`
(`exit_z < entry_z`). Confiança de entrada = `|z| / (2 * entry_z)`,
saturada em `[0, 1]`; saída sempre `1.0` (fato binário, não força
graduada).

**Quant Momentum** (`QuantMomentumStrategy`) — mesma premissa da baseline
`MomentumStrategy` (tendência persiste), mas medindo tendência pela
inclinação de uma regressão linear rolling (`regression.slope`) em vez do
retorno ponta-a-ponta da baseline (sensível a ruído nos dois preços
extremos da janela). Só entra quando `regression.r_squared >=
min_r_squared` (a reta explica boa parte do movimento — não é um
ziguezague que por acaso terminou mais alto) **e** `relative_volume >=
min_relative_volume` (participação acima da média confirma que a
tendência é "real"). Uma vez posicionada, sai quando `r_squared` cai
abaixo de `exit_r_squared` (tendência não é mais limpa) **ou** `slope`
inverte de sinal (tendência acabou/reverteu). Confiança de entrada =
`r_squared` diretamente — já normalizado em `[0, 1]` pela própria
definição estatística; saída sempre `1.0`.

**Volatility Breakout** (`VolatilityBreakoutStrategy`) — hipótese oposta à
de reversão à média: um rompimento das Bollinger Bands (`percent_b > 1.0`
ou `< 0.0`) é lido como início de tendência, não como esticado demais —
mas só quando confirmado por volatilidade em expansão
(`bollinger.bandwidth` maior que no candle anterior — comparação com um
valor de feature lembrado do candle anterior, não um indicador
recalculado) **e** `relative_volume >= min_relative_volume`. Um
rompimento com volatilidade contraindo ou volume abaixo da média é lido
como possível fakeout, não confirmado. Uma vez posicionada, sai quando o
preço reentra nas bandas (`0.0 <= percent_b <= 1.0` de novo) — o
rompimento perdeu força. Confiança de entrada = excesso de `percent_b`
além da borda da banda, dividido por `0.5` e saturado em `[0, 1]`; saída
sempre `1.0`.

Todas as três: (a) mantêm cada baseline correspondente inalterada, lado a
lado, para comparação; (b) declaram sua própria `feature_config()` — as
janelas que usam, nada implícito ou compartilhado; (c) fecham sua própria
posição sozinhas, via `SignalDirection::Flat` — nunca dependem de outra
estratégia emitir um `Short` nem de stop loss/take profit externo (ver
ADR-17 para a auditoria semântica que motivou isso, e a limitação de
rastreamento de posição documentada em cada arquivo: a estratégia nunca vê
a `RiskDecision` real, então assume que toda entrada que emitiu foi
aprovada); (d) têm um teste determinístico de não-look-ahead
(`no_look_ahead_decision_at_shared_index_is_unaffected_by_future_candles`
em cada arquivo) que roda o mesmo prefixo de candles isoladamente e dentro
de uma série mais longa, e verifica que a decisão no índice compartilhado
não muda; (e) `confidence` é força heurística do sinal, nunca uma
probabilidade calibrada de lucro — o motor de risco nunca a lê para
dimensionar a ordem (`risk::engine::tests::signal_confidence_does_not_affect_position_sizing`);
(f) não fazem otimização automática de parâmetros — os defaults em cada
`*Params::default()` são escolhas convencionais documentadas, não
ajustadas por busca a nenhum instrumento ou período específico.

## 7c. Strategy Library — catálogo e descoberta

`strategies::catalog` (Fase 1 do futuro Strategy Selector — ver ADR
correspondente) é o ponto único de "quais estratégias este projeto
oferece": `catalog::entries()` devolve as 6, cada uma com um
`StrategyDescriptor` (nome legível, descrição, categoria
baseline/quantitativa, `StrategyRequirements`, direções suportadas,
parâmetros default serializados) e uma fábrica que constrói uma instância
nova a partir de um `StrategyId` fornecido pelo chamador —
`catalog::build(kind, id)` ou, para o caso comum de hoje (id igual ao
kind), `catalog::build_default(kind)`.

Isso não substitui `StrategyRegistry`: o catálogo responde "o que
existe" (estático, sem estado), o registry responde "o que está rodando
agora nesta execução, para quais instrumentos" (o dispatcher de
`on_event`). `crates/app/src/setup.rs::build_strategy_registry` e
`crates/experiments/src/strategy_set.rs` são os dois consumidores hoje —
nenhum dos dois mantém sua própria lista de estratégias/parâmetros
separada.

`catalog::build` aceitar um `StrategyId` arbitrário (em vez de sempre
derivá-lo do `kind`) é o que deixa a arquitetura pronta para múltiplas
instâncias independentes da mesma estratégia (ex.: duas instâncias de
`quant_momentum` com ids diferentes rodando ao mesmo tempo) sem
implementar nenhum mecanismo de execução para isso ainda — só a
capacidade de construir.

### 7c.1. `strategies::instance` — múltiplas instâncias por dado (Fase 1.5)

`strategies::instance::build_registry(configs, instruments, available_market_data)`
transforma uma lista de `StrategyInstanceConfig { id, kind, symbols }` num
`StrategyRegistry` populado, usando `catalog::build` internamente — nenhuma
combinação estratégia↔instrumento hardcoded no chamador. Uma instância é
`id` (o "robot_id" — o mesmo `StrategyId` que já aparece em todo
`Signal`/`Position`, suficiente para comparar PnL/trades por instância sem
nenhum campo novo em `domain`) + `kind` do catálogo + os símbolos que ela
deve negociar. Duas instâncias com `id`s diferentes (mesmo `kind` ou não)
já rodam concorrentemente sem compartilhar estado — cada uma é uma
`Box<dyn Strategy>` independente.

`app::setup::build_strategy_registry` monta essa lista a partir de
`AppConfig`: se `STRATEGY_INSTANCES` (formato
`id:kind:SIMBOLO[+SIMBOLO...]`, instâncias separadas por `;` — ex.
`qm-btc-a:quant_momentum:BTC/USDT;qm-btc-b:quant_momentum:BTC/USDT`) estiver
configurado, usa-o tal como está; senão cai no comportamento anterior à
Fase 1.5 (uma instância por kind de `ENABLED_STRATEGIES`, id igual ao kind,
aplicada a todo instrumento compatível por classe de ativo) — nenhum
deploy existente muda de comportamento só por esta capacidade existir.

`backtest::BacktestRunner` não precisou de nenhuma mudança para isso: ele
já recebia um `&mut StrategyRegistry` pré-montado e despachava para
qualquer número de entradas — múltiplas instâncias em backtest já
funcionam só passando um registry construído por `build_registry`.

**Limite estrutural conhecido, não resolvido nesta fase:**
`portfolio::PortfolioManager`/`risk::RiskEngine` permitem no máximo **uma
posição aberta por instrumento, não por (instrumento, estratégia)** —
`open_position_for`/`RejectionReason::PositionAlreadyOpen` são
indexados só por `InstrumentId`. Duas instâncias configuradas para o
mesmo símbolo continuam recebendo eventos e produzindo sinais
independentemente, mas se ambas tentarem manter uma posição aberta ao
mesmo tempo no mesmo instrumento, a segunda a entrar é rejeitada pelo
Risk Engine — não porque a arquitetura de estratégias não suporte
múltiplas instâncias, mas porque o modelo de portfólio/risco ainda não
distingue "de quem" é a posição além do `strategy_id` que ela carrega
depois de aberta. Isso já é uma limitação pré-existente (as 3 baselines
default já concorrem pelo mesmo instrumento hoje) — Fase 1.5 não a piora,
só a torna mais visível. Resolver isso (se necessário) é uma decisão de
Risk/Portfolio, deliberadamente fora do escopo desta fase.

## 8. Como criar uma nova estratégia

1. Decida onde ela pertence: `strategies/generic/` apenas se o cálculo for
   genuinamente agnóstico à classe de ativo; caso contrário,
   `strategies/crypto/` ou `strategies/equities/`.
2. Implemente `strategies::Strategy`:
   ```rust
   pub trait Strategy: Send {
       fn id(&self) -> &StrategyId;
       fn requirements(&self) -> &StrategyRequirements;
       fn on_event(&mut self, instrument: &Instrument, event: &MarketEvent) -> Option<Signal>;
   }
   ```
   Mantenha o estado por instrumento em um `HashMap<InstrumentId, ...>`
   dentro da sua struct — uma instância pode ser registrada contra vários
   instrumentos (veja `EmaCrossoverStrategy` para o padrão).
3. Escreva um `StrategyRequirements` honesto: declare apenas
   `supported_asset_classes` sobre os quais você realmente raciocinou, e
   apenas os `required_market_data` de que seu `on_event` realmente
   precisa.
4. Documente, em um doc comment no nível do módulo, exatamente como as três
   estratégias genéricas existentes: racional, entradas, parâmetros,
   mercados suportados, dados necessários, limitações e quando *não* usá-la.
   Isso não é opcional — é o que impede que a estratégia seja aplicada
   indiscriminadamente depois.
5. Adicione testes unitários determinísticos (veja
   `crates/strategies/src/generic/ema_crossover.rs` para o padrão:
   construir um `Instrument` sintético, alimentar uma série de preços feita
   à mão, verificar os sinais resultantes).
6. Adicione uma entrada em `strategies::catalog::entries()`
   (`crates/strategies/src/catalog.rs`) — id, nome legível, descrição,
   categoria, direções suportadas e a fábrica de instância. É a única
   lista que precisa saber da estratégia nova: `app` (via
   `ENABLED_STRATEGIES`) e `experiments` já a descobrem a partir dali,
   sem nenhuma mudança própria. Veja [§7c](#7c-strategy-library--catálogo-e-descoberta).
7. Nunca faça suposições de risco, execução ou persistência dentro da
   estratégia — veja [§6](#6-princípios-arquiteturais), regra 4.

## 9. Como adicionar uma nova exchange de cripto

1. Adicione um novo módulo em `crates/market-data/src/`, por exemplo
   `coinbase/`, espelhando o formato interno de `binance/`: `dto` (tipos de
   fio privados), `normalize` (conversão privada de JSON -> `domain`),
   `rest`, `ws`, `provider`.
2. Implemente `market_data::MarketDataProvider` para o novo tipo de
   provider. Seu `capabilities()` deve reportar honestamente o que a
   exchange pode fornecer (`MarketDataKind`s, `AssetClass`es).
3. Nunca deixe os formatos JSON da nova exchange escaparem do novo módulo —
   apenas tipos de `domain` cruzam a fronteira de `MarketDataProvider`.
4. Reexporte o novo tipo de provider a partir de
   `crates/market-data/src/lib.rs` (veja como `BinanceMarketData` é
   reexportado).
5. Faça o wiring em `crates/app/src/setup.rs` / `main.rs` — hoje esses
   módulos fixam `BinanceMarketData` no código; suportar múltiplos
   providers simultâneos significaria generalizar esse wiring (não a API
   pública do crate), por exemplo tornando `provider` um
   `Box<dyn MarketDataProvider>` escolhido por configuração.
6. Nada em `domain`, `strategies`, `risk`, `portfolio`, `execution`,
   `persistence`, `analytics` ou `backtest` deveria precisar mudar.

## 10. Como adicionar ações no futuro

**Não implementado nesta fase — por design.** Adicionar ações deveria
exigir, em princípio, apenas:

1. Um `MarketDataProvider` de ações (por exemplo, encapsulando a API
   REST/WebSocket de um fornecedor), seguindo o mesmo formato de adaptador
   de `binance/`.
2. Uma implementação de `TradingCalendar` correspondente (por exemplo,
   `B3TradingCalendar`, `NyseTradingCalendar`) — `domain::TradingCalendar`
   já existe como trait; apenas `AlwaysOpenCalendar` (cripto) está
   implementado hoje. Estratégias nunca checam o relógio por conta própria;
   quem as aciona (o loop ao vivo ou o runner de backtest) é que consulta o
   calendário.
3. Um adaptador de broker, se/quando execução real em ações for adicionada
   (uma nova implementação de `execution::Broker` — `PaperBroker` já
   funciona para ações em simulação, já que é agnóstico à classe de ativo).
4. Metadados de instrumento vindos do novo provider (tick size, lot size,
   mínimos) — exatamente o mesmo formato de `Instrument` que cripto usa
   hoje.
5. Estratégias específicas de ações, apenas onde genuinamente necessárias,
   em `strategies/equities/`.

Estes devem ser reutilizados sem alteração: `domain`, o trait e o registry
do motor de estratégias, `risk`, `portfolio`, `PaperBroker`, `persistence`,
`analytics`, `backtest`. **Se adicionar ações algum dia exigir mudanças
profundas nesses crates, isso é sinal de acoplamento indevido introduzido
em algum ponto do caminho — corrija o acoplamento, não o contorne.**

## 11. Modelo de domínio

Tudo em `crates/domain/src/`, sem dependências de infraestrutura:

| Tipo | Responsabilidade |
|---|---|
| `Asset` | Um código de moeda/ativo validado e em maiúsculas (`BTC`, `USDT`, ...). |
| `Symbol` | Par `BASE/QUOTE` legível por humanos — nunca uma string nativa de exchange como `BTCUSDT`. |
| `AssetClass` | `Crypto \| CryptoDerivative \| Equity \| Future \| Forex` — o único enum universal entre mercados. |
| `Exchange` | Identidade do venue (`Binance`, `Other(String)`). |
| `MarketType` | Estrutura do order book: `Spot \| Perpetual \| DatedFuture \| Equity`. |
| `MarketDataKind` | Um tipo de dado de mercado que um provider pode fornecer / uma estratégia pode exigir (`Ohlcv`, `Trades`, `OrderBookL1/L2`, `FundingRate`, `OpenInterest`, `TradingSession`, `OnChain`). |
| `Instrument` / `InstrumentId` | Metadados completos do instrumento negociável: símbolo, ativos base/cotação, classe de ativo, exchange, market type, tick size, lot size, quantidade mínima, notional mínimo. Também é dono de `round_price_to_tick`/`round_quantity_down_to_lot` — o único lugar onde a quantização de tick/lot é implementada (veja ADR-10). O `InstrumentId` é atribuído aleatoriamente por `Instrument::new`, mas tornado estável entre reinícios por `persistence::instruments::upsert` (ADR-9), não pelo próprio `Instrument`. |
| `Price` / `Quantity` / `Money` | Newtypes baseados em `rust_decimal`; `Price`/`Quantity` são estritamente positivos por construção. |
| `Timeframe` | Período de agregação do candle (`M1`..`W1`). |
| `Candle` | Barra OHLCV, normalizada a partir do que a exchange enviou. |
| `MarketTrade` | Um único tick executado. |
| `OrderBookSnapshot` / `BookLevel` | Snapshot do book L1/L2. |
| `MarketEvent` | `Candle \| Trade \| OrderBook` — o tipo de evento compartilhado entre ao vivo e backtest. |
| `Side` | `Buy \| Sell`. |
| `Signal` / `SignalDirection` / `SignalId` / `StrategyId` | A opinião de uma estratégia — não uma ordem. |
| `OrderRequest` / `Order` / `OrderStatus` / `OrderType` / `Fill` / `Liquidity` | Intenção e ciclo de vida da ordem. |
| `Position` / `PositionStatus` | Uma posição mantida (ou fechada), com P&L realizado uma vez fechada. |
| `PortfolioSnapshot` | Estado do portfólio em um ponto no tempo, para persistência/analytics. |
| `TradingCalendar` / `AlwaysOpenCalendar` | Controle de sessão de mercado, aplicado fora das estratégias. |

## 12. Banco de dados

PostgreSQL via SQLx. As migrations ficam em `/migrations` na raiz do
repositório (aplicadas por `persistence::run_migrations`, seguras de chamar
a cada inicialização).

| Tabela | Propósito |
|---|---|
| `instruments` | Metadados de instrumentos negociáveis, espelhados dos dados do provider. `(symbol, exchange, market_type)` é a chave natural que o `upsert` usa para manter o `id` estável entre reinícios — veja ADR-9. |
| `strategy_configs` | Uma linha por `StrategyId` configurado: tipo, parâmetros, requisitos declarados. |
| `signals` | Todo sinal que uma estratégia emite, aprovado ou não — para auditabilidade. |
| `orders` | Intenções de ordem e seu ciclo de vida. |
| `fills` | Eventos individuais de fill de uma ordem, incluindo `spread_cost`/`slippage_cost` separados de `fee` (veja ADR-11). |
| `positions` | Estado atual + histórico das posições (abertas e fechadas), incluindo `spread_paid`/`slippage_paid` cumulativos ao lado de `fees_paid`. |
| `trades` | Ledger append-only de trades de ida e volta concluídos (uma linha por posição fechada), mantido separado de `positions` para consultas de analytics que só se importam com o histórico fechado. Carrega a mesma quebra de `spread_paid`/`slippage_paid`, além de `order_id` (a ordem de fechamento) para que a timeline de `web` possa unir uma decisão diretamente ao P&L resultante — veja ADR-12. |
| `portfolio_snapshots` | Equity/caixa/exposição/`realized_pnl_today` em um ponto no tempo. Escrito uma vez por candle fechado (não amostrado), já que [§14b](#14b-ui-web-de-observabilidade) lê a linha mais recente como "estado atual" — veja ADR-12. |
| `strategy_performance` | Rollups periódicos de desempenho por estratégia. Atualmente nada escreve nela — a view de desempenho de `web` calcula isso sob demanda a partir de `positions` (veja ADR-12); esta tabela está reservada para se/quando for necessário um histórico escrito da métrica ao longo do tempo. |
| `latest_prices` | Preço de marcação atual por instrumento, atualizado via upsert a cada candle fechado — existe apenas para que `web` (um processo separado) possa mostrar o "preço atual" sem compartilhar memória com `app`. Não é histórico de ticks. |
| `risk_decisions` | Toda decisão do motor de risco — aprovada ou rejeitada — com seu motivo, o `trigger` (`signal`/`stop_loss`/`take_profit`) e (se aprovada) o `order_id` resultante. É o único lugar onde um sinal *rejeitado* deixa qualquer rastro; sem ela, a timeline de `web` poderia mostrar aprovações, mas nunca explicar uma rejeição. Veja ADR-12. |

Deliberadamente **não** armazenado no Postgres: histórico bruto de
OHLCV/ticks em escala. Veja [§16](#16-limitações-atuais) para o caminho
planejado baseado em Parquet quando o backtesting precisar de dados
históricos em massa.

## 13. Configuração

Variáveis de ambiente, carregadas do `.env` via `dotenvy` (veja
`.env.example` para a lista completa com defaults e comentários):

| Variável | Obrigatória | Propósito |
|---|---|---|
| `DATABASE_URL` | sim | String de conexão do Postgres. |
| `PAPER_INITIAL_BALANCE` | não (default `100000`) | Caixa virtual inicial. |
| `PAPER_MAKER_FEE` / `PAPER_TAKER_FEE` | não | Frações de fee do PaperBroker. |
| `PAPER_SPREAD_BPS` / `PAPER_SLIPPAGE_BPS` | não | Spread e slippage simulados, cada um em basis points, modelados e registrados separadamente (veja ADR-11). |
| `ENABLED_SYMBOLS` | não (default 3 pares) | Lista `BASE/QUOTE` separada por vírgula. |
| `ENABLED_STRATEGIES` | não (default as 3 baselines: `ema_crossover,momentum,mean_reversion`) | Tipos de estratégia separados por vírgula — qualquer id de `strategies::catalog::entries()` (as 6, incluindo as 3 quantitativas) é aceito; o default não inclui as quantitativas de propósito, ver [§7c](#7c-strategy-library--catálogo-e-descoberta). Ignorada se `STRATEGY_INSTANCES` estiver configurado. |
| `STRATEGY_INSTANCES` | não (default vazio) | Instâncias explícitas de estratégia (Fase 1.5), formato `id:kind:SIMBOLO[+SIMBOLO...]` separadas por `;` — permite múltiplas instâncias independentes do mesmo `kind` e escolher exatamente quais instrumentos cada uma negocia. Ver [§7c.1](#7c1-strategiesinstance--múltiplas-instâncias-por-dado-fase-15). |
| `WARMUP_CANDLES` | não (default `50`) | Candles históricos a carregar antes de ir ao vivo. |
| `RISK_ORDER_NOTIONAL`, `RISK_MAX_POSITION_NOTIONAL`, `RISK_MAX_TOTAL_EXPOSURE`, `RISK_MAX_OPEN_POSITIONS`, `RISK_STOP_LOSS_PCT`, `RISK_TAKE_PROFIT_PCT`, `RISK_MAX_DAILY_LOSS` | não | Campos de `RiskConfig`. |
| `RUST_LOG` | não (default `info`) | Sintaxe do `EnvFilter` do `tracing-subscriber`. |
| `WEB_PORT` | não (default `58080`) | Porta em que o `quant-engine-web` ([§14b](#14b-ui-web-de-observabilidade)) escuta, apenas em `127.0.0.1`. Não é lida pelo próprio `quant-engine`. Default deliberadamente incomum (não `8080`) para evitar colisão com outro dev server local. |

Nenhum segredo está fixado no código em lugar algum; a única variável
obrigatória é `DATABASE_URL`, e todo endpoint da Binance utilizado é
público/não autenticado.

O diretório `config/` na raiz do repositório está reservado para
configuração estruturada futura (perfis de risco por ambiente, arquivos de
parâmetros por estratégia) — nada o lê ainda; veja `config/README.md`.

## 14. Como executar

Requer Rust (stable, edição 2021, testado na 1.95) e PostgreSQL.

```bash
# 1. Suba o Postgres (ou aponte DATABASE_URL para um que você já tenha)
docker compose up -d

# 2. Configure
cp .env.example .env
# edite o .env se você mudou as credenciais/porta do Postgres

# 3. Rode — as migrations são aplicadas automaticamente na inicialização
cargo run --bin quant-engine
```

Você deve ver logs estruturados para: registro de instrumentos, registro de
estratégias (ou avisos de compatibilidade), carga de candles de warmup, e
então um fluxo de linhas `market event: candle closed` e — sempre que uma
estratégia disparar — linhas `strategy signal`, `risk decision:
approved/rejected`, `paper order executed`, `position opened/closed`.
Pressione Ctrl+C para um shutdown gracioso; um snapshot final de portfólio
e um resumo de desempenho da sessão são logados antes da saída.

### 14b. UI web de observabilidade

Um dashboard web local e **somente leitura** para acompanhar o que o
`quant-engine` está fazendo sem ler logs — voltado a alguém que não é
especialista em Rust/trading e quer responder, de relance: *o que o sistema
está fazendo, por que tomou cada decisão e está ganhando ou perdendo
dinheiro?*

Ele **não tem capacidade de execução alguma** — nenhum botão de
compra/venda, nenhuma rota que mute qualquer coisa, nenhum cálculo
financeiro próprio. Ele apenas consulta o Postgres (via `persistence`) e,
onde um número precisa ser calculado, chama exatamente as mesmas funções que
o pipeline de trading já usa (`domain::Position::unrealized_pnl`,
`analytics::compute_performance`) — veja o ADR-12 para por que essa
fronteira importa.

```bash
# Em um segundo terminal, junto com `cargo run --bin quant-engine`:
cargo run -p web
# -> http://127.0.0.1:58080
```

Ele precisa da mesma `DATABASE_URL` que o `quant-engine` (lê o mesmo
`.env`) e pode ser iniciado antes, depois ou de forma independente dele —
não há acoplamento em memória entre os dois processos, apenas o banco
Postgres compartilhado. Rodá-lo sem nenhuma atividade de trading ainda é
tranquilo; as seções apenas mostram uma mensagem de estado vazio até que
`app` tenha escrito algo.

Abra `http://127.0.0.1:58080` em um navegador. A página consulta sua própria
API JSON (`/api/overview`, `/api/prices`, `/api/positions`, `/api/trades`,
`/api/performance`, `/api/timeline`) a cada 5 segundos. O frontend é um SPA
React (Tailwind + shadcn/ui + Animate UI) buildado em `crates/web/static` e
servido pelo próprio Axum — as rotas `/api/*` continuam sendo o contrato
estável; o React só apresenta dados, sem lógica financeira. Para
reconstruir a UI após mudanças em `crates/web/frontend`:

```bash
cd crates/web/frontend && npm install && npm run build
```

Pensada para alguém sem conhecimento técnico de trading conseguir usar: a
visão principal responde em poucos segundos *o que o robô está fazendo
agora* (paper trading, patrimônio, resultado, capital exposto, posições,
última decisão); detalhes técnicos ficam sob demanda (tooltips "?", cards
expansíveis, sheet, tabs). Todo termo não óbvio (saldo, patrimônio, P&L
realizado/não realizado, drawdown, exposição, confiança, fee, spread,
slippage, profit factor, taxa de acerto etc.) tem explicação em português
simples. A "confiança" de um sinal é descrita honestamente pelo que ela
realmente é hoje — a força do próprio indicador da estratégia normalizada
entre 0 e 100%, não uma probabilidade estatística de lucro nem resultado de
backtest — para não sugerir uma precisão que o sistema não tem. Mensagens
de runtime vindas do motor de risco (motivos de rejeição, gatilhos de stop
loss/take profit) são traduzidas para português em duas camadas, nenhuma
delas tocando `risk::RejectionReason` nem seu `Display` (que continuam em
inglês para logs/testes internos): na escrita,
`app::pipeline::translate_rejection_reason`
grava toda decisão nova já em português; na leitura,
`web::i18n::translate_legacy_reason` reconhece o formato exato do
`Display` de `risk::RejectionReason` via parsing de string (sem depender do
crate `risk`) e traduz qualquer linha que ainda esteja em inglês no
Postgres — rede de segurança para o que foi persistido antes do primeiro
fix existir, sem precisar alterar dados históricos. Ela mostra:

- **O que o robô está fazendo agora** — resumo no topo, em linguagem
  simples: que está em Paper Trading e dinheiro real não é usado, quantas
  posições estão abertas, quanto capital está exposto agora (e que fração
  do patrimônio isso representa), o P&L em aberto, e a última decisão
  relevante com seu motivo. Montado a partir dos mesmos dados de
  `/api/overview`, `/api/positions` e `/api/timeline` já buscados pelas
  outras seções — nenhum endpoint novo.
- **Visão geral** — caixa, equity, P&L realizado diário/total, P&L não
  realizado, retorno acumulado, max drawdown, número de posições abertas,
  exposição.
- **Preços monitorados** — último preço conhecido por instrumento.
- **Posições abertas** — preço de entrada, preço atual, P&L não realizado,
  estratégia, tempo em aberto.
- **Desempenho por estratégia** — as mesmas métricas que `analytics`
  calcula para o portfólio inteiro, agrupadas por `strategy_id`.
- **Histórico de trades** — entrada, saída, P&L bruto, fees, spread,
  slippage, P&L líquido, por trade fechado.
- **Timeline** — uma entrada por decisão do motor de risco, com as mais
  antigas ocultas (as 100 mais recentes são exibidas): o sinal disparador
  (direção, confiança) ou o trigger de saída forçada, se foi aprovada ou
  rejeitada e por quê, e — quando aprovada — o fill resultante (preço, fee,
  custo de spread, custo de slippage) e, se fechou uma posição, o P&L. Este
  é o único lugar em que um sinal *rejeitado* é visível; ele nunca produz
  uma ordem, então sem essa view não deixaria rastro algum em nenhuma parte
  do sistema.

`strategy_id`s de testes de integração/smoke test (prefixo `smoke-test-` ou
`restart-test-`, usados por `crates/app/tests/`) são excluídos em duas
camadas, não só na exibição:

- **Na origem**: `app::setup::restore_portfolio` (chamada no bootstrap do
  `quant-engine`, antes de processar qualquer evento) exclui essas posições
  ao reconstruir o `PortfolioManager` da instância ao vivo — elas nunca
  entram em `closed_positions`/`open_positions` do ledger real, então
  `realized_pnl_total`/`realized_pnl_today` da própria instância (e todo
  `portfolio_snapshots` que ela grava daí em diante) já nascem corretos.
  Causa raiz confirmada em auditoria (2026-08-24): sem essa exclusão, o P&L
  sintético de `crates/app/tests/portfolio_restart.rs` (compra a 50000,
  venda a 51000 — milhares de unidades monetárias, muito acima de qualquer
  trade real) inflava "P&L total realizado"/"P&L diário" no dashboard a um
  valor sem relação com a soma real por estratégia mostrada em Desempenho.
  `cash`/`equity`/`return_pct`/`exposure_ratio` nunca foram afetados por
  esse bug especificamente: derivam só do caixa rastreado incrementalmente
  e das posições abertas, nunca de uma agregação de `closed_positions` —
  por isso o patrimônio já parecia plausível mesmo com o P&L realizado
  inflado.
- **Na leitura** (`web::handlers::is_test_strategy`, mesmo padrão de
  prefixo): Posições abertas, Desempenho, Histórico de trades e Timeline
  filtram de novo ao consultar o Postgres, e a Visão geral recalcula
  `realized_pnl_total`/`realized_pnl_today`/`unrealized_pnl`/
  `open_positions_count` a partir de posições já filtradas em vez de
  confiar direto no `portfolio_snapshots` mais recente — assim o dashboard
  mostra o número certo imediatamente, sem depender de a instância ao vivo
  já ter reiniciado com o fix acima.

## 15. Como testar

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Os três passam limpos atualmente (156 testes unitários/de componente, 0
falhas). Eles usam entradas determinísticas do início ao fim (séries de
preços feitas à mão, Decimals fixos) — sem dependência de rede ao vivo ou
de acesso a banco de dados.

Existem dois testes de integração adicionais marcados com `#[ignore]` por
padrão (excluídos do comando acima) porque precisam de infraestrutura real
— Postgres via Docker e, para um deles, acesso de rede à Binance:

```bash
docker compose up -d
cargo test -p app --test smoke_test -- --ignored --nocapture
cargo test -p persistence --test instrument_identity -- --ignored --nocapture
cargo test -p app --test portfolio_restart -- --ignored --nocapture --test-threads=1
```

O `smoke_test` prova o caminho completo — dados reais da Binance →
estratégia → sinal → risco → execução em paper → portfólio → Postgres real
— buscando candles ao vivo de BTC/USDT, anexando um movimento de preço
sintético determinístico para disparar um sinal de estratégia, rodando isso
pelo event loop real de `pipeline::run` e então consultando o Postgres de
volta para confirmar que cada tabela (`instruments`, `signals`, `orders`,
`fills`, `positions`, `portfolio_snapshots`) foi de fato escrita.

O `instrument_identity` prova a estabilidade do `InstrumentId` (ADR-9):
fazer upsert da mesma chave natural duas vezes, cada uma com um id
recém-gerado diferente por `Instrument::new`, deve retornar o mesmo id
autoritativo nas duas vezes.

O `portfolio_restart` prova a recuperação de estado no restart (ADR-13):
uma posição aberta sobrevive a um "restart" simulado do `quant-engine`
(mesma posição, não recriada), o Risk Engine enxerga esse estado restaurado
e recusa uma segunda entrada no mesmo instrumento, e — auditoria de
2026-08-24 — `restore_portfolio` exclui posições de `strategy_id` de teste
(`smoke-test-*`/`restart-test-*`) do ledger da instância ao vivo, para que
elas nunca contaminem `realized_pnl_total`/`realized_pnl_today` (veja a
causa raiz documentada em `app::setup::is_test_strategy`). Precisa de
`--test-threads=1`: suas três funções escrevem em `portfolio_snapshots`/
`positions`, tabelas globais compartilhadas por todos os testes deste
binário — sem isso, uma pode ler um estado transitório escrito pela outra
e falhar por uma inconsistência que não é real (não é um bug de produção,
é uma característica de testar contra um Postgres real e persistente em
vez de um mock).

## 16. Limitações atuais

Declaradas explicitamente, não escondidas:

- **Nenhum caminho de execução real existe ou está habilitado.** Veja
  [§17](#17-segurança).
- **Sem venda a descoberto.** O spot de cripto não tem mecanismo de
  aluguel/margem, então `risk::RiskEngine` nunca aprova abrir uma posição a
  partir de um sinal `Short`, e `execution::PaperBroker` recusa
  independentemente abrir uma, mesmo se solicitado diretamente — veja a
  entrada do crate `risk` acima e o ADR-8 em `docs/architecture.md`. Toda
  posição que existe é `Side::Buy`.
- **Sem ações.** Dados, calendários e qualquer estratégia específica de
  ações são trabalho futuro (veja
  [§10](#10-como-adicionar-ações-no-futuro)).
- **O PaperBroker só executa ordens `OrderType::Market`.** Simulação de
  limit/maker exigiria modelar um order book em repouso, o que esta fase
  não tenta.
- **Aproximações de exposição e equity.** `risk::PortfolioState` e
  `portfolio::PortfolioManager::risk_snapshot` aproximam a exposição usando
  o colateral pelo preço de entrada, não o notional a preço de mercado ao
  vivo, para evitar exigir um preço atualizado de cada instrumento aberto
  no momento da avaliação do sinal.
- **`analytics::PerformanceReport::max_drawdown` é uma aproximação baseada
  na sequência de trades** (pico a vale do P&L acumulado de trades
  fechados), não um drawdown verdadeiro da curva de equity — veja o doc
  comment do campo.
- **Ainda não há métricas de Sharpe, Sortino, expectância, volatilidade ou
  correlação entre estratégias.** Elas precisam de uma série temporal de
  retornos que este crate ainda não consome do histórico de snapshots do
  `PortfolioManager`.
- **Sem armazenamento de dados históricos em massa.** Apenas linhas
  operacionais recentes vivem no Postgres; um backtest apropriado sobre
  meses de candles de 1 minuto precisaria de um data lake baseado em
  Parquet (ou similar), que ainda não existe.
- **Existem dois testes de integração** (`crates/app/tests/smoke_test.rs`:
  dados reais da Binance -> estratégia -> sinal -> risco -> execução em
  paper -> portfólio -> Postgres real;
  `crates/persistence/tests/instrument_identity.rs`: estabilidade do
  `InstrumentId`, ADR-9), ambos com `#[ignore]` por padrão, já que precisam
  do Postgres via Docker (e, para o primeiro, de acesso de rede) — veja
  [§15](#15-como-testar) para os comandos de execução. Ainda não há uma
  suíte de integração de persistência mais ampla (por exemplo, casos de
  borda por função de repositório) além desses dois caminhos.
- **Modelo de uma posição por instrumento.** O motor de risco rejeita uma
  segunda entrada em um instrumento que já tem posição aberta — sem
  piramidação nem reversão no lugar.
- **`persistence` usa queries SQLx verificadas em runtime, não as macros
  `query!` de tempo de compilação** — um tradeoff deliberado (veja os docs
  de módulo do crate) para evitar exigir um banco de dados ativo ou um
  cache offline de queries no momento do `cargo build`.
- **Spread e slippage são ambos modelados como ajustes fixos em bps sobre o
  preço de referência**, não como um modelo real de profundidade/impacto do
  order book. Eles são separados, configurados e registrados individualmente
  (`PaperBrokerConfig::spread_bps`/`slippage_bps`,
  `Fill::spread_cost`/`slippage_cost`,
  `Position::spread_paid`/`slippage_paid` — veja os docs de módulo de
  `execution::PaperBroker` e o ADR-10 em `docs/architecture.md`), mas
  nenhum depende do tamanho da ordem, da liquidez ou do estado real do book.
- **O arredondamento de tick/lot presume metadados precisos da exchange.**
  `risk` quantiza o preço para `tick_size` e a quantidade para `lot_size`
  *antes* de revalidar `min_notional` contra esses valores arredondados (de
  modo que uma quantidade que só atinge o mínimo antes do arredondamento
  seja corretamente rejeitada depois), mas ele ainda confia em quaisquer
  `tick_size`/`lot_size`/`min_notional` que o adaptador da exchange
  reportou — não há verificação independente junto à exchange.
- **A UI web não tem autenticação e escuta apenas em `127.0.0.1`, por
  design** — ela é feita para ser visualizada na mesma máquina em que o
  `quant-engine` roda. Não a coloque atrás de uma porta pública sem antes
  adicionar autenticação; hoje não existe nenhuma.
- **A validação de consistência do `PortfolioManager::restore` é global,
  não por instrumento.** No bootstrap, `app::setup::restore_portfolio`
  (ver ADR-13) carrega *todas* as posições abertas persistidas de uma vez
  e rejeita o startup inteiro se encontrar qualquer inconsistência em
  qualquer instrumento (ex.: mais de uma posição aberta para o mesmo
  instrumento) — não apenas o instrumento afetado. Isso é intencional
  (falhar alto é melhor que seguir com um estado que pode estar errado),
  mas significa que uma linha corrompida em `positions` bloqueia o
  restart do sistema inteiro até ser corrigida manualmente no banco, não
  só o instrumento problemático.

## 17. Segurança

**Este sistema não deve executar ordens reais hoje.** `PaperBroker` é a
única implementação de `Broker`, é totalmente simulado e não tem caminho de
código para a API de envio de ordens de nenhuma exchange real.
`market-data::BinanceMarketData` só chama os endpoints *públicos* e não
autenticados da Binance e não tem capacidade alguma de envio de ordens.

Habilitar execução com dinheiro real no futuro deve ser uma decisão
explícita e deliberada — uma nova implementação de `Broker`, gestão real de
credenciais e, quase certamente, um rollout em etapas (veja
[§18](#18-roadmap), Fase 6) — e não um efeito colateral incidental de uma
mudança não relacionada. Nenhum contribuidor ou assistente de IA deve
conectar o envio real de ordens sem que essa decisão tenha sido tomada
explicitamente pelo dono do projeto.

## 18. Roadmap

```text
Fase 1 — Fundação em cripto            (este repositório, hoje)
Fase 2 — Melhor execução em paper      (fills limit/maker, fills parciais, simulação de order book)
Fase 3 — Backtesting                   (dados históricos em massa / Parquet, replay mais rico, walk-forward)
Fase 4 — Pesquisa de estratégias       (mais indicadores, feature store, rastreio de hipóteses)
Fase 5 — Paper trading prospectivo     (validação de longa duração, analytics mais ricos: Sharpe/Sortino/expectância)
Fase 6 — Execução real limitada em cripto  (decisão explícita necessária — veja §17)
Fase 7 — Adaptadores de ações          (veja §10)
```

## 19. Guia de desenvolvimento com IA

Se você é um assistente de IA trabalhando neste repositório, leia esta
seção por inteiro antes de fazer mudanças.

**Objetivo do projeto.** Uma base de paper trading pequena, correta,
testável e extensível para pesquisa quant em cripto — não uma plataforma de
hedge fund, não um framework para todo mercado possível. Prefira a solução
simples que preserva a evolução futura em vez da sofisticada, a menos que a
solução simples crie acoplamento estrutural real.

**Decisões arquiteturais que você não deve quebrar de forma leviana:**

- `Signal != Order`. Nunca deixe uma estratégia construir um `OrderRequest`
  ou chamar `execution`/`risk` diretamente.
- `Strategy != Risk`. Estratégias leem `MarketEvent`s e retornam
  `Option<Signal>` — nada além disso. Limites de risco, dimensionamento de
  posição e regras de saída pertencem a `risk`.
- `MarketDataProvider != domain`. Nenhum JSON/struct específico de exchange
  pode cruzar para fora de seu módulo adaptador (por exemplo,
  `market-data::binance::dto` deve permanecer privado).
- `PaperBroker != Adaptador de Exchange`. Não junte ingestão de dados de
  mercado e execução de ordens em um único crate ou tipo.
- `portfolio::PortfolioManager` é o **único** dono do estado de
  caixa/posições. Não deixe `risk` ou `execution` manterem uma segunda
  cópia — eles leem um snapshot ou chamam através dos próprios métodos de
  `PortfolioManager`.
- `AssetClass` continua sendo um enum pequeno e universal. Não adicione
  campos a ele, e não adicione uma variante que não seja respaldada por um
  mercado real que este projeto de fato modele.
- A compatibilidade de estratégias é imposta por
  `strategies::check_compatible` / `StrategyRegistry::register`, no momento
  do registro. Nunca adicione um caminho que despache um evento a uma
  estratégia sem passar antes pelo registro.
- `web` é **somente leitura** e permanece assim, a menos que o dono do
  projeto explicitamente decida o contrário. Nenhuma rota pode mutar nada,
  enviar uma ordem ou aceitar um parâmetro financeiro vindo de uma
  requisição. Nenhum cálculo financeiro pode ser escrito *dentro* de `web`
  — reutilize funções de `domain`/`analytics` (veja ADR-12); se um número
  de que a UI precisa não tem função existente que o calcule, isso é sinal
  de que a função deve ser adicionada onde o cálculo pertence (`domain`,
  `portfolio`, `analytics`), não inline em um handler.

**Overengineering a evitar:**

- Não adicione traits "por arquitetura" com uma única implementação e sem
  necessidade concreta de uma segunda.
- Não introduza generics, macros ou `Arc<Mutex<...>>` a menos que um
  requisito real e presente os exija.
- Não construa um sistema de plugins, uma DSL ou um framework de
  configuração além do que `.env` + `crates/app/src/config.rs` já oferecem,
  a menos que solicitado.
- Não implemente ações antecipadamente "já que você está por aqui". Veja
  [§10](#10-como-adicionar-ações-no-futuro) — isso está deliberadamente não
  implementado.

**Sobre estratégias e resultados:**

- Nenhuma estratégia neste repositório é presumida lucrativa. Não escreva
  comentários de código, mensagens de log ou documentação que sugiram o
  contrário.
- Toda nova estratégia deve documentar racional, entradas, parâmetros,
  mercados suportados, dados necessários, limitações e quando não usá-la —
  veja [§8](#8-como-criar-uma-nova-estratégia).
- Trate a saída de backtest/paper trading como amostras estatísticas. Não
  fabrique números de desempenho, e não adicione métricas de analytics sem
  um cálculo real por trás delas (veja o doc de módulo de `analytics` para
  exatamente quais métricas estão e quais não estão implementadas, e por
  quê).

**Sobre execução:**

- Dinheiro real jamais deve ser habilitado sem uma decisão explícita e
  separada do dono do projeto — veja [§17](#17-segurança). Se uma tarefa
  parecer pedir envio real de ordens, pare e confirme em vez de
  implementá-la.

**Mantendo este documento atualizado:** quando você tomar uma decisão
arquitetural que mude algo descrito acima — um novo crate, uma fronteira
alterada, uma métrica recém-implementada, um caminho de execução recém
habilitado — atualize este README na mesma mudança. Futuros assistentes de
IA (e humanos) dependem de ele ser preciso, não aspiracional.
