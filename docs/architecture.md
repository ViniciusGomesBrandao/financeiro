# Decisões de arquitetura

ADRs em formato curto para decisões que seriam caras de errar ou fáceis de
desfazer sem querer. Para o panorama completo (mapa de crates, fluxo de
dados, princípios), comece pelo [README.md](../README.md) da raiz — este
arquivo registra apenas o *porquê*, para o punhado de decisões
suficientemente não óbvias a ponto de precisarem disso.

## ADR-1: Workspace Cargo, binário único — não microsserviços

**Decisão.** Um `cargo run` inicia tudo: ingestão de dados de mercado,
avaliação de estratégias, risco, execução em paper e persistência. Sem fila
de mensagens, sem service mesh, sem deployables separados.

**Por quê.** No estágio deste projeto (uma base de pesquisa/paper trading,
não uma mesa de trading em produção), um único processo é mais simples de
executar, depurar e raciocinar sobre, e um workspace Cargo já entrega
fronteiras limpas entre crates sem o custo operacional de um sistema
distribuído. Dividir em serviços é uma decisão de escala cuja necessidade
este projeto ainda não conquistou — veja o §18 (Roadmap) do README raiz para
o que justificaria revisitá-la.

## ADR-2: `AssetClass` é a única abstração universal entre mercados

**Decisão.** O core (`domain`, `strategies::Strategy`, `risk::RiskEngine`,
`portfolio::PortfolioManager`, `execution::Broker`) conhece exatamente um
conceito transversal a mercados: `AssetClass`. Todo o resto que difere entre
cripto e ações (sessões de negociação, funding rates, convenções de tick
size, liquidação) é empurrado para código específico de mercado — adaptadores
de market data, módulos de estratégia `crypto/`/`equities/`, implementações
de `TradingCalendar`.

**Por quê.** A instrução mais explícita que dá forma a este projeto: não
force a estratégia de um mercado a rodar em outro mercado abstraindo
diferenças reais. Uma interface `MarketDataProvider` ou um trait `Strategy`
que tentasse representar as peculiaridades de todo mercado possível iria (a)
crescer de forma enorme e vazar detalhes, ou (b) esconder silenciosamente
premissas que só valem para cripto. Manter pequena a superfície compartilhada
é o que torna "adicionar ações depois" plausível — veja o §10 do README e o
ADR-4 abaixo.

## ADR-3: `Signal`, `Order` e `Position` são três tipos diferentes

**Decisão.** Uma `Strategy` emite um `Signal` (uma opinião, sem quantidade,
sem aprovação). O `RiskEngine` transforma um `Signal` aprovado em um
`OrderRequest` (uma intenção). O `Broker` transforma um `OrderRequest` em um
`Order` + `Fill` e — via `PortfolioManager` — em uma `Position`.

**Por quê.** Colapsar esses tipos (por exemplo, fazendo estratégias emitirem
"ordens" diretamente) tornaria impossível: auditar o que uma estratégia
*queria* versus o que o risco *permitiu*; reutilizar o mesmo código de
estratégia contra uma hipotética configuração futura de risco; ou impedir que
implementações de `Strategy` dependam do estado da conta. Essa separação é o
que permite que os princípios 3–4 do §6 do README sejam fatos aplicáveis, e
não aspirações.

## ADR-4: `MarketEvent` é compartilhado entre os caminhos ao vivo e de backtest

**Decisão.** `domain::MarketEvent` (`Candle | Trade | OrderBook`) é
construído tanto pelo pipeline ao vivo (`app::pipeline`, alimentado pelo
stream WebSocket de `market-data`) quanto por `backtest::BacktestRunner`
(alimentado por `Candle`s históricos). Ambos então chamam a *mesma* sequência
`StrategyRegistry::dispatch` / `RiskEngine::evaluate` /
`Broker::submit_order`.

**Por quê.** A alternativa — um tipo de evento exclusivo de backtest ou um
caminho paralelo de avaliação de estratégias — arrisca fazer os caminhos de
código ao vivo e de backtest divergirem silenciosamente, o que tornaria os
resultados de backtest inúteis como previsão do comportamento ao vivo.
Compartilhar o tipo e a sequência de chamadas é uma garantia estrutural, não
apenas uma convenção.

## ADR-5: Contabilidade de posição por colateral, tanto para `Buy` quanto para `Sell`

**Decisão.** `PortfolioManager::open_position` debita do caixa
`entry_price * quantity + fee` independentemente do `Side`;
`close_position` credita de volta esse mesmo colateral mais/menos o P&L
realizado. Posições long e short são contabilizadas de forma idêntica.

**Por quê.** Modelar a mecânica real de venda a descoberto (disponibilidade
de aluguel, chamadas de margem, taxas de aluguel) está fora do escopo de uma
base de paper trading e adicionaria complexidade substancial sem valor de
pesquisa neste estágio. A contabilidade uniforme de colateral é simples,
simétrica e suficiente para validar a lógica das estratégias e a correção do
pipeline — que é o objetivo real desta fase (veja §1–2 do README raiz).
Revisitar se/quando a Fase 6 (execução real) for escopada.

## ADR-6: `persistence` usa queries SQLx verificadas em runtime, não `query!`

**Decisão.** Toda query em `crates/persistence` usa
`sqlx::query`/`sqlx::query_as` (verificada em runtime contra o schema real),
nunca as macros de tempo de compilação `sqlx::query!`/`query_as!`.

**Por quê.** As macros de tempo de compilação exigem ou um banco de dados
ativo alcançável no momento do `cargo build`, ou um cache offline `.sqlx`
commitado e mantido em sincronia com toda mudança de migration. Exigir um
Postgres ativo (ou disciplina de manutenção de cache) para que o `cargo
build` funcione foi julgado um default pior para o estágio deste projeto —
inclusive para assistentes de IA e ambientes de CI que podem não ter Postgres
disponível — do que abrir mão da checagem de tipos de coluna em tempo de
compilação. Revisitar se o schema estabilizar e um Postgres em CI se tornar
padrão.

## ADR-7: Endpoints públicos do Binance Spot como única fonte de dados de mercado

**Decisão.** `market-data::binance::BinanceMarketData` chama apenas os
endpoints REST/WebSocket públicos e não autenticados da Binance (klines
recentes, `exchangeInfo`, streams combinados de kline/trade). Nenhuma API key
é usada ou exigida em qualquer lugar deste repositório.

**Por quê.** Esta fase precisa de dados OHLCV/trade reais e atuais para
validar o pipeline — ela não precisa de endpoints privados (dados de conta,
envio de ordens). Permanecer não autenticado mantém o projeto executável por
qualquer pessoa com zero configuração de conta, e mantém "não existe
capacidade de execução real" (§17 do README raiz) verdadeiro por construção,
não apenas por convenção.

## ADR-8: Sem venda a descoberto, aplicado em `risk` e `execution`, não em `domain`/`portfolio`

**Decisão.** `risk::RiskEngine::evaluate` nunca aprova a abertura de uma nova
posição a partir de um sinal `Short` — um `Short` só é acionável quando pode
fechar um long existente (economicamente uma saída, tratada de forma idêntica
a `Flat`); sem posição aberta, ele é rejeitado com
`RejectionReason::ShortSellingNotSupported`. `execution::PaperBroker` recusa
independentemente abrir uma posição a partir de uma ordem do lado `Sell`
quando o portfólio não detém nada daquele instrumento
(`BrokerError::NakedShortNotSupported`), como uma segunda proteção
independente.

Nem `domain::Position` nem `portfolio::PortfolioManager` impõem isso:
`Position::unrealized_pnl` ainda calcula corretamente para uma hipotética
posição `Side::Sell`, e `PortfolioManager::open_position`/`close_position`
ainda contabilizam qualquer um dos lados simetricamente (veja ADR-5).

**Por quê.** Este projeto opera cripto **spot** — não há aluguel nem margem,
então vender um ativo que o portfólio não detém não é uma simplificação a ser
adiada, é impossível no mercado que está sendo modelado. Existem duas
verificações independentes (risco *e* execução) porque `risk` é o portão
normal, mas em princípio não é o único chamador de `Broker::submit_order` (um
futuro backtest, script ou teste poderia chamar o broker diretamente); o
broker não deve confiar em um chamador upstream como sua única proteção
contra um trade impossível.

Aplicar isso em `risk`/`execution` em vez de em `domain`/`portfolio` mantém
essas duas camadas fazendo aquilo para o que existem: `domain::Position` é uma
estrutura de dados simples utilizável por qualquer tipo de venue futuro
(incluindo uma fase de margem ou derivativos que *precisaria* de
contabilidade simétrica long/short), e a escrituração de
`portfolio::PortfolioManager` continua reutilizável em vez de ter a regra de
negócio spot-only de hoje gravada dentro dela. Se/quando uma fase de margem ou
derivativos for escopada, apenas `risk`/`execution` deveriam precisar mudar
aqui — `domain` e `portfolio` não.

## ADR-9: O Postgres é a autoridade sobre `InstrumentId`

**Decisão.** `domain::Instrument::new` ainda atribui um `InstrumentId`
aleatório novo a cada chamada — isso não mudou. O que mudou:
`persistence::instruments::upsert` agora retorna o id que o Postgres
considera autoritativo para a chave natural daquele instrumento
(`(symbol, exchange, market_type)`, a constraint `UNIQUE` já existente na
tabela), via `INSERT ... ON CONFLICT (...) DO UPDATE ... RETURNING id`. Em uma
primeira inserção de verdade, esse é o id recém-gerado pelo chamador; em toda
chamada subsequente para a mesma chave natural (inclusive um novo processo
gerando ainda outro id aleatório), é o id que já estava registrado. Todo
chamador que persiste um `Instrument` recém-buscado — hoje, apenas
`app::setup::load_instruments` — é obrigado a sobrescrever o
`Instrument::id` em memória com esse valor de retorno antes de usá-lo para
qualquer outra coisa (despachar eventos, dimensionar ordens, logar), e não
apenas antes de persistir.

**Por quê.** `Instrument::new` não pode ser determinístico por si só sem
alcançar o Postgres de dentro de `domain` (que precisa permanecer livre de
infraestrutura — veja os docs de módulo do próprio crate) ou sem assumir uma
dependência de que o construtor não tem por que precisar. A alternativa —
adicionar uma busca `find_by_natural_key` separada que o chamador precisa
lembrar de chamar *antes* de cada `upsert` — exige os mesmos dois round trips
em mais lugares e é mais fácil de esquecer. Dobrar a busca dentro do próprio
upsert (`RETURNING id`) faz de "o id retornado é sempre correto" uma
propriedade de simplesmente chamar `upsert`, e não uma convenção que os
chamadores têm de sustentar à parte. O custo: todo chamador precisa lembrar
que o id que passou *para dentro* não é necessariamente o id que volta *para
fora* — veja `crates/app/tests/smoke_test.rs` e
`crates/persistence/tests/instrument_identity.rs` para o padrão e sua
cobertura de testes.

## ADR-10: O arredondamento de tick/lot vive em `domain::Instrument`, aplicado tanto no sizing quanto na execução

**Decisão.** `Instrument::round_price_to_tick`/`round_quantity_down_to_lot`
são métodos simples em `domain::Instrument`, que recebem um `Decimal` bruto e
retornam um quantizado. `risk::RiskEngine::evaluate_entry` arredonda para
baixo, até o lot size, a quantidade que dimensiona, antes que qualquer uma de
suas próprias checagens de limite rode (de modo que as verificações de
exposição/notional/caixa raciocinem sobre a quantidade que pode de fato ser
executada). `execution::PaperBroker::submit_order` arredonda
independentemente a quantidade da ordem recebida para baixo, até o lot size,
*de novo*, e arredonda o preço de execução que ele próprio calcula para o tick
size.

**Por quê.** A *regra* de arredondamento (como quantizar) só precisa existir
uma vez, e `Instrument` é o tipo que já é dono de `tick_size`/`lot_size` — uma
função livre em outro lugar apenas alcançaria os campos de `Instrument` de
fora, sem benefício. Onde a regra é *aplicada* é necessariamente dividido:
`risk` é o único lugar que dimensiona uma ordem nova (quantidade), e
`execution` é o único lugar que calcula o preço do fill (após
spread/slippage), então cada um arredonda o valor que só ele produz. O fato de
`execution` arredondar a quantidade de novo, mesmo que `risk` já tenha feito
isso, é a mesma postura de defesa em profundidade da proteção contra naked
short no ADR-8: o broker não confia em seu chamador como único ponto de
imposição. Arredondar quantidade é sempre para *baixo* (`.floor()`), nunca
para o mais próximo, especificamente porque arredondar para cima poderia
empurrar uma ordem aprovada pelo risco além do notional ou do caixa que foi de
fato aprovado — arredondar para baixo nunca pode.

## ADR-11: Spread e slippage são separados, ambos diagnósticos sobre o preço

**Decisão.** `PaperBrokerConfig` tem dois parâmetros de ajuste adverso,
`spread_bps` e `slippage_bps`, aplicados aditivamente ao preço de referência
(mesma direção, mesma fórmula, apenas duas frações independentes somadas antes
de serem aplicadas) em vez de um único `slippage_bps` combinado. A magnitude
por unidade resultante de cada um é registrada em `Fill` como
`spread_cost`/`slippage_cost` e acumulada em `Position` (entrada + saída) como
`spread_paid`/`slippage_paid`, espelhando o campo `fees_paid` já existente.

Fundamentalmente, `spread_cost`/`slippage_cost`/`spread_paid`/`slippage_paid`
são **apenas diagnósticos** — nunca são subtraídos do caixa nem do
`realized_pnl_net` uma segunda vez. Ambos já estão embutidos no próprio
`price` do fill (é isso que "ajuste adverso" significa), então o P&L calculado
a partir da diferença entre preços de entrada/saída já os reflete. Apenas
`fee` é um custo que *não* está implícito em uma diferença de preço, e é por
isso que `fee` — e somente `fee` — é uma subtração separada em
`portfolio::PortfolioManager::open_position`/`close_position`.

**Por quê.** A tarefa que motivou isso (auditar se o P&L líquido contabiliza
fees + spread + slippage) revelou duas necessidades distintas que um único
campo `default_slippage_bps` não conseguiria satisfazer: primeiro, poder
afirmar *quanto* do preço adverso de um fill veio de cruzar o spread versus de
slippage adicional no estilo impacto de mercado, para relatórios futuros;
segundo, garantir que adicionar essa visibilidade não cobrasse do trader em
dobro por acidente, subtraindo um custo que já estava refletido no preço.
Modelar ambos como frações fixas do preço de referência (sem depender do
tamanho da ordem ou da liquidez) mantém a simulação exatamente tão simples
quanto era antes desta mudança — veja o §16 do README raiz para a limitação
resultante de que nenhum dos dois se aproxima da economia real do order book.

## ADR-12: A UI de observabilidade é um segundo processo, acoplado apenas via Postgres

**Decisão.** `crates/web` é um binário separado (`quant-engine-web`), não uma
feature flag nem uma task em background criada dentro de `quant-engine`. Ele
não compartilha nada em memória com o pipeline de trading — nenhum canal,
nenhum `Arc<Mutex<...>>`, nenhum `PortfolioManager` compartilhado. Sua única
relação com `app` é que ambos se conectam ao mesmo banco Postgres; `web` nunca
importa `market-data`, `strategies`, `risk`, `execution` ou `portfolio`, e tem
exatamente uma capacidade de escrita em todo o seu grafo de dependências:
nenhuma — todo handler é uma leitura.

Tornar isso verdadeiro de ponta a ponta exigiu algumas pequenas mudanças
aditivas de schema e de pipeline, todas reutilizando um cálculo existente em
vez de inventar um novo:
- `latest_prices` (nova tabela) — o pipeline já mantém um mapa `mark_prices`
  em memória a cada candle fechado; isso apenas persiste o mesmo valor para
  que um processo separado possa ler o "preço atual" sem compartilhar essa
  memória.
- `risk_decisions` (nova tabela) — antes disso, um sinal *rejeitado* não
  deixava rastro em lugar nenhum: `signals` registra o sinal, mas nada
  registrava por que `risk::RiskEngine` disse não. Sem essa tabela não há como
  mostrar "sinal → motivo → decisão" para uma rejeição, do que depende toda a
  premissa da UI.
- `trades.order_id` (nova coluna) — permite que uma linha da timeline
  (chaveada por `risk_decisions.order_id`) faça join direto até o P&L do trade
  que aquela ordem fechou, em vez de correlacionar por
  instrumento/timestamp heuristicamente.
- `portfolio_snapshots.realized_pnl_today` (nova coluna) — persiste um valor
  que `PortfolioManager::realized_pnl_today` já calculava (ele alimenta a
  checagem de perda diária do motor de risco); o "P&L diário" na UI é esse
  valor, não um novo cálculo.
- Snapshot por candle em vez de a cada 20 candles — a UI sempre lê "o snapshot
  mais recente" como estado atual (veja abaixo), então snapshots infrequentes
  deixariam saldo/equity visivelmente desatualizados entre atualizações da UI.

Nada disso toca a lógica de decisão de `risk`, a lógica de fill de
`execution` ou a escrituração de `portfolio` — toda adição é ou uma nova
tabela na qual um caminho de código existente escreve mais uma linha, ou um
valor que já existia sendo levado um passo adiante.

Os handlers de `web` deliberadamente não recalculam estado financeiro. Onde um
número exige cálculo (P&L não realizado de uma posição aberta, max drawdown,
desempenho por estratégia), o handler chama a mesma função que
`risk`/`portfolio`/`analytics` já usam
(`domain::Position::unrealized_pnl`, `analytics::compute_performance`) — ele
nunca reimplementa a aritmética. Números de visão geral e por estratégia que
não precisam de recálculo ao vivo (caixa, equity, exposição, retorno) são
lidos diretamente da linha mais recente de `portfolio_snapshots`, que o
`PortfolioManager` — o único dono desse estado — já produziu.

**Por quê.** Dois processos lendo um banco compartilhado, em vez de um único
processo com um servidor web embutido lendo sua própria memória, foi escolhido
por três motivos. Primeiro, isso mantém a garantia de "nenhuma capacidade de
execução" estrutural em vez de uma questão de disciplina: `web` fisicamente
não consegue chamar `Broker::submit_order` porque não depende de `execution`
de forma alguma, em vez de meramente escolher não chamar uma função à qual tem
acesso. Segundo, isso significa que a UI pode ser iniciada, parada ou
reiniciada de forma independente do pipeline de trading — útil para uma
ferramenta local que alguém pode querer deixar aberta em uma aba do navegador
ao longo de várias execuções do `quant-engine`. Terceiro, evita a superfície de
concorrência que um design de memória compartilhada introduziria (travar um
`PortfolioManager` vivo a partir de um segundo runtime assíncrono) para uma
funcionalidade que é explicitamente somente leitura e tolerante à pequena
latência de um round trip extra ao Postgres. O custo são as adições de schema
acima — julgadas válidas frente a inventar, no lugar disso, um mecanismo
in-process de pub/sub ou de estado compartilhado.

## ADR-13: Restauração do portfólio no bootstrap, com Postgres como fonte da verdade

**Decisão.** `app::setup::restore_portfolio` roda no bootstrap do
`quant-engine`, antes de abrir o stream de market data e antes de
qualquer estratégia processar um evento. Ela busca as posições abertas
(`persistence::positions::list_open`), as posições fechadas
(`persistence::positions::list_closed`) e o snapshot mais recente
(`persistence::portfolio_snapshots::latest`), e usa
`PortfolioManager::restore` para montar o `PortfolioManager` a partir
desses dados — em vez de `PortfolioManager::new`, que sempre começa vazio.

O caixa atual vem diretamente do `cash` do último snapshot persistido, não
de um recálculo a partir do histórico de posições: esse valor já foi
calculado corretamente, uma vez, pelo mesmo `PortfolioManager` que o
escreveu antes do restart, então reaproveitá-lo evita duplicar a
aritmética de custo/fee em outro lugar. Posições abertas e fechadas são
carregadas como estão — mesmo `id`, mesmos `fees_paid`/`spread_paid`/
`slippage_paid` — nunca recriadas.

`PortfolioManager::restore` valida duas invariantes antes de aceitar os
dados: nenhuma posição "aberta" pode ter `status != Open` (idem para
fechadas), e no máximo uma posição aberta é permitida por instrumento — a
mesma invariante que `risk::RiskEngine` já assume ao verificar
`PositionAlreadyOpen`. Qualquer violação retorna
`PortfolioError::InconsistentRestore` em vez de silenciosamente escolher
uma posição ou ignorar a duplicata. `restore_portfolio` trata a ausência
de snapshot como um erro fatal *apenas* quando já existem posições
abertas persistidas (nesse caso não há como saber o caixa atual com
segurança); um banco totalmente vazio — a primeira execução — não é
tratado como inconsistência, e o portfólio simplesmente começa do
`initial_cash` configurado.

**Por quê.** Antes desta mudança, reiniciar o `quant-engine` com uma
posição aberta fazia o Risk Engine acreditar que o portfólio estava vazio
— o instrumento voltava a aceitar uma *nova* entrada, e a posição antiga
persistida ficava órfã em relação ao novo estado em memória, sem que nada
detectasse isso. Restaurar o estado a partir do Postgres no bootstrap (a
mesma fonte de verdade que `web`, ADR-12, já usa para observabilidade)
elimina essa janela — sem precisar de nenhum mecanismo de persistência
novo, já que `positions` e `portfolio_snapshots` já existiam exatamente
para isso.

A validação explícita (em vez de tolerar dados inconsistentes) segue o
mesmo princípio de "falhar alto" já usado em outros pontos do sistema
(ex.: `check_compatible` para estratégias incompatíveis, ADR-8 para short
selling em spot): um portfólio que começa com um número errado de
posições abertas pode levar o Risk Engine a aprovar ou rejeitar ordens
incorretamente sem que ninguém perceba — melhor recusar o startup e
expor o problema.

## ADR-14: Feature Engine como crate separado, sem look-ahead por construção, sem integração com estratégias existentes

**Decisão.** `crates/features` calcula indicadores derivados de OHLCV
(retornos multi-horizonte, SMA/EMA, desvio padrão, volatilidade realizada,
volatilidade EWMA, z-score, Bollinger Bands, ATR, RSI, momentum/ROC,
regressão linear rolling com slope e R², autocorrelação, volume relativo)
como um crate próprio, dependendo apenas de `domain` — o mesmo padrão de
isolamento já usado por `analytics`. `FeatureEngine` é escopado a um único
instrumento e expõe uma única operação, `update(&Candle) ->
Option<FeatureSnapshot>`, pensada para ser alimentada um candle fechado por
vez, em ordem cronológica — o mesmo contrato que
`strategies::Strategy::on_event`/`StrategyRegistry::dispatch` já usam no
loop ao vivo e no backtest. `backtest::BacktestRunner::run` foi estendido
(não reescrito) para manter um `FeatureEngine` por instrumento durante a
reprodução e devolver os snapshots resultantes em
`BacktestReport::feature_snapshots`, na mesma ordem cronológica da
reprodução. As três estratégias genéricas existentes
(`strategies::generic::{ema_crossover, momentum, mean_reversion}`) **não**
foram alteradas para consumir `FeatureSnapshot` — continuam calculando seus
próprios indicadores internamente via `strategies::generic::indicators`.

**Por quê.**

*Sem look-ahead bias por construção, não por disciplina.* A alternativa
mais óbvia — uma função `compute_features(all_candles, index) ->
FeatureSnapshot` chamada com o histórico inteiro e um índice — depende de
quem chama nunca passar um índice além do que "deveria" estar disponível
naquele instante; um erro de off-by-one silenciosamente vaza informação do
futuro para dentro de um backtest, inflando sua performance aparente sem
nenhum sintoma visível. Modelar o motor como uma máquina de estado
incremental (`update` consome exatamente um candle, mantém só o estado
necessário — janelas rolantes, EMAs, médias de Wilder — e nunca vê nada
além do que já foi alimentado) torna essa classe de bug estruturalmente
impossível de cometer no motor em si: não há "índice" para errar. A prova
disso é um teste (`no_look_ahead_snapshot_at_index_i_is_unaffected_by_future_candles`,
replicado em `backtest::runner::tests` através do `BacktestRunner` real)
que reproduz o mesmo prefixo de candles duas vezes — uma vez sozinho, uma
vez seguido de candles futuros adicionais — e verifica que o snapshot já
produzido na primeira execução é bit-a-bit idêntico nas duas.

*`f64`, não `Decimal`.* Mesma decisão e mesmo motivo já documentado em
`strategies::generic::indicators::Ema`: estes são valores estatísticos
derivados (médias, desvios, correlações, regressões), não dinheiro — a
exatidão decimal de `rust_decimal` não é necessária, e `f64` mantém raiz
quadrada, regressão e EWMA simples. A conversão de `Decimal` (como os
candles armazenam OHLCV) para `f64` acontece uma única vez, na borda de
`FeatureEngine::update`.

*Não integrado às estratégias existentes, de propósito.* Migrar
`ema_crossover`/`momentum`/`mean_reversion` para consumir
`FeatureSnapshot` em vez de recalcular seus próprios indicadores exigiria
mudar a assinatura de `Strategy::on_event` (hoje recebe só `&Instrument` +
`&MarketEvent`) para também receber features, uma mudança que atravessa
toda estratégia existente e o `StrategyRegistry`. Esse refactor é
desejável eventualmente — evita duas implementações de EMA divergindo com
o tempo — mas é ortogonal a construir o motor em si, e misturar as duas
coisas no mesmo trabalho arriscaria alterar o comportamento de estratégias
já validadas por engano. `features` foi construído para que essa migração
futura seja possível (o `FeatureSnapshot` já cobre os indicadores que as
três estratégias atuais usam), não para forçá-la agora.

*OHLCV e microestrutura não são a mesma coisa.* Um livro de ordens
(`domain::OrderBookSnapshot`) ou uma sequência de trades individuais
(`domain::MarketTrade`) carregam informação que candles agregados não
capturam — profundidade, desequilíbrio de fluxo de ordens, tamanho médio
de trade. Um futuro módulo de features de microestrutura deve viver como
um módulo (ou crate) **irmão** de `features::ohlcv`, com seu próprio
`FeatureSnapshot`, em vez de virar campos "às vezes preenchidos" dependendo
da fonte de dados disponível — decisão documentada no próprio doc do crate
raiz de `features`, para que a tentação de "só adicionar um campo" numa
extensão apressada não a viole silenciosamente mais tarde.

## ADR-15: `FeatureStrategy` como trait separado de `Strategy`, adaptado via `FeatureStrategyAdapter`

**Decisão.** Estratégias que consomem `features::FeatureSnapshot` não
implementam `strategies::Strategy` diretamente. Elas implementam um trait
novo e menor, `strategies::FeatureStrategy` (`id`, `requirements`,
`feature_config`, `on_features(&Instrument, &FeatureSnapshot) ->
Option<Signal>`), que não tem nenhum método recebendo `MarketEvent`/
`Candle`. `FeatureStrategyAdapter<S: FeatureStrategy>` é o único código que
faz a ponte: implementa `Strategy` por cima de um `S: FeatureStrategy`,
mantendo um `features::FeatureEngine` por instrumento (`HashMap
<InstrumentId, FeatureEngine>`) e traduzindo cada `MarketEvent::Candle`
fechado em uma chamada a `engine.update` seguida de `inner.on_features`. O
`Strategy` original — usado pelas três estratégias baseline
(`ema_crossover`, `momentum`, `mean_reversion`) — não foi alterado.

**Por quê.**

*A interface pequena é o mecanismo de imposição, não só um estilo.* A
alternativa mais simples seria dar às estratégias baseadas em features
acesso direto a `MarketEvent` (implementando `Strategy` normalmente) e
confiar que elas "vão preferir" ler de um `FeatureSnapshot` calculado em
algum outro lugar. Isso não impede nada de verdade: nada barra uma
implementação de recalcular sua própria EMA ali dentro, exatamente o
problema que motivou este trabalho ("estratégias consomem features, não
recalculam indicadores"). Retirar `MarketEvent`/`Candle` do trait por
completo torna o recálculo estruturalmente impossível de escrever, não
apenas desencorajado por convenção — uma implementação de `FeatureStrategy`
literalmente não tem de onde ler um preço bruto, só o `FeatureSnapshot` já
pronto que `on_features` recebe.

*Duas gerações lado a lado, sem migrar as baselines.* Migrar
`ema_crossover`/`momentum`/`mean_reversion` para `FeatureStrategy` exigiria
trocar sua fonte de indicadores por `FeatureEngine` e validar que o
comportamento não mudou — um trabalho ortogonal a introduzir o mecanismo em
si, e arriscado de fazer junto (uma estratégia já validada podendo mudar de
comportamento por engano). Um trait novo, coexistindo com `Strategy`, evita
essa escolha: as baselines continuam exatamente como estavam, e as
estratégias novas (`statistical_mean_reversion`, `quant_momentum`,
`volatility_breakout`, ver ADR-16) usam o caminho novo desde o início.

*O adapter, não cada estratégia, sabe sobre `FeatureEngine`.* Sem
`FeatureStrategyAdapter`, cada uma das três estratégias novas precisaria
repetir o mesmo par `HashMap<InstrumentId, FeatureEngine>` +
`entry().or_insert_with(...)` + "ignorar eventos não-candle/não-fechados"
que já existe uma vez, corretamente, no adapter. Generalizar esse código
uma única vez (`FeatureStrategyAdapter<S>`) em vez de deixar cada
implementação de `FeatureStrategy` reescrevê-lo mantém a interface "enxuta"
pedida: o que uma estratégia nova precisa escrever é só a lógica de
decisão (`on_features`) e a configuração de janelas (`feature_config`),
nada de bookkeeping de ciclo de vida.

*Continua sem look-ahead, por composição.* `FeatureStrategyAdapter::on_event`
alimenta `FeatureEngine::update` um candle fechado por vez, na mesma ordem
em que `StrategyRegistry::dispatch` os entrega — a mesma garantia
estrutural do ADR-14, agora composta com o fato de que `on_features` só
recebe um snapshot por chamada, sem acesso a nenhum outro estado externo. A
prova disso são os testes
`no_look_ahead_decision_at_shared_index_is_unaffected_by_future_candles`
em cada uma das três estratégias novas, que reproduzem o mesmo prefixo de
candles isoladamente e dentro de uma série mais longa e comparam a decisão
(direção + confiança, não o `Signal` inteiro — `Signal::id` é um UUID novo
a cada chamada de `Signal::new`, então comparar o `Signal` inteiro entre
duas execuções separadas sempre falharia por um motivo que não tem nada a
ver com look-ahead).

## ADR-16: Três estratégias quantitativas combinam features por filtro conjuntivo, nunca por peso

**Decisão.** `StatisticalMeanReversionStrategy`, `QuantMomentumStrategy` e
`VolatilityBreakoutStrategy` (`crates/strategies/src/generic/`) usam cada
uma, no máximo, três features de `FeatureSnapshot`. Em cada caso, uma
feature é o **gatilho** (decide direção e é a única fonte da confiança do
sinal) e as demais são **filtros binários** (confirmam ou vetam a entrada;
nunca entram numa soma ponderada). Nenhuma das três estratégias tem um
coeficiente, peso ou fator de escala inventado sem justificativa
estatística direta.

**Por quê.** Combinar múltiplos indicadores numa pontuação ponderada (ex.:
`score = 0.4*zscore + 0.3*rsi + 0.3*autocorr`) é a forma mais comum de uma
estratégia "quantitativa" parecer mais sofisticada do que é: os pesos
raramente vêm de uma derivação real, só de tentativa e erro até os
backtests ficarem bons — exatamente o tipo de ajuste que otimização
automática de parâmetros faria de propósito (fora do escopo deste
trabalho) e que um humano faz por acidente ao "só tentar uns números". Um
filtro conjuntivo evita essa armadilha estruturalmente: cada feature de
confirmação tem uma pergunta binária clara e justificável por si só ("RSI
confirma sobrecompra?", "a tendência tem R² alto o bastante para confiar
nela?", "a volatilidade está se expandindo, não contraindo?"), e a
resposta é sim/não, não "quanto peso isso deveria ter". A confiança do
sinal vem sempre de uma única variável já naturalmente escalada (magnitude
de z-score, R² — já em `[0,1]` pela própria definição estatística,
distância além de uma banda), nunca de uma combinação das três.

Cada combinação tem uma justificativa própria, documentada no topo de cada
arquivo de estratégia: z-score (desvio de preço) e RSI (proporção
ganho/perda) medem coisas relacionadas mas não idênticas, então exigir os
dois reduz falsos positivos de um extremo isolado; autocorrelação detecta
o regime em que a premissa de reversão à média deixa de valer (tendência
persistente); R² separa uma tendência "limpa" de um slope inflado por
ruído; volume relativo confirma participação real por trás de um
movimento de preço, tanto para momentum quanto para breakout; e
`bandwidth` em expansão distingue um rompimento genuíno de um "fakeout" em
contração de volatilidade. Nenhuma dessas quatro justificativas depende
das outras três — é isso que torna a combinação "justificável" em vez de
arbitrária.

## ADR-17: Auditoria semântica das três estratégias quantitativas — saída própria, timing sinal→execução, confiança não-probabilística

**Contexto.** Depois de introduzir as três estratégias quantitativas
(ADR-16), uma auditoria semântica dedicada verificou três propriedades que
código sintaticamente correto pode violar silenciosamente: (1) uma ordem
nunca pode executar antes de o sinal que a gerou existir; (2) cada
estratégia precisa saber fechar sozinha o que ela mesma abre; (3)
`Signal::confidence` precisa continuar sendo só uma força heurística, nunca
algo que o dimensionamento de posição trate como probabilidade. A auditoria
confirmou (1) e (3) já válidos pela arquitetura existente, e encontrou (2)
genuinamente ausente — corrigido nesta mesma auditoria.

**(1) Timing sinal → execução — verificado, não alterado.** `backtest::
BacktestRunner::run` e `app::pipeline::run` processam candles estritamente
em ordem cronológica, um de cada vez; dentro de uma iteração,
`FeatureEngine::update` → `StrategyRegistry::dispatch` →
`RiskEngine::evaluate` → `Broker::submit_order` acontecem na mesma
passagem, sempre nessa ordem — não há caminho de código em que uma ordem
seja submetida antes do sinal que a origina existir, nem que um sinal do
candle T seja aplicado a um candle anterior. Isso já era verdade antes
desta auditoria (é a mesma disciplina do ADR-14) e vale igualmente para as
três estratégias baseline e as três novas, já que todas passam pelo mesmo
`registry.dispatch`. Adicionado um teste de prova direta,
`backtest::runner::tests::signal_execution_never_precedes_the_candle_that_produced_it`,
que roda `QuantMomentumStrategy` através do `BacktestRunner` real duas
vezes — uma vez sem o candle que dispara a entrada (nenhuma posição deve
existir) e uma vez com ele incluído (a posição deve existir, com
`opened_at` exatamente igual ao `close_time` daquele candle, nem antes nem
adiado para um candle futuro).

**(2) Saída própria e explícita — ausente, corrigida.** Antes desta
auditoria, as três estratégias novas só sabiam *entrar* — nenhuma emitia
`SignalDirection::Flat` nem qualquer outro sinal de fechamento. Uma
posição aberta por `StatisticalMeanReversionStrategy`, por exemplo, só
seria fechada se: (a) um stop loss/take profit estivesse configurado em
`RiskConfig` (uma saída de risco, não da estratégia), ou (b) *outra*
estratégia registrada no mesmo instrumento por acaso emitisse um `Short`
(que `risk::RiskEngine::evaluate` trata como fechamento quando já existe
posição — ver ADR-8). Isso é exatamente a falha que a auditoria pediu para
verificar: a estratégia dependendo, silenciosamente, de outra para se
fechar. Corrigido dando a cada uma das três um critério de saída derivado
da sua própria hipótese de entrada — nunca um valor novo e arbitrário:

- `StatisticalMeanReversionStrategy`: sai quando `|zscore| <= exit_z`
  (reverteu à média o suficiente — o complemento direto do gatilho de
  entrada, `|zscore| >= entry_z`).
- `QuantMomentumStrategy`: sai quando `r_squared < exit_r_squared` (a
  tendência não é mais limpa) ou `slope` inverte de sinal em relação à
  posição (a tendência acabou/reverteu).
- `VolatilityBreakoutStrategy`: sai quando `percent_b` volta para
  `[0, 1]` (o preço reentrou nas bandas — o complemento direto do gatilho
  de entrada).

Cada estratégia agora rastreia, por instrumento, o lado da posição que
*acredita* ter aberto (`HashMap<InstrumentId, domain::Side>`) — memória da
própria decisão, não um indicador recalculado (a mesma categoria de estado
interno que `VolatilityBreakoutStrategy` já usava para `prev_bandwidth`).
Enquanto rastreia uma posição, `on_features` avalia *só* a saída, nunca
uma nova entrada — garantindo que a estratégia nunca tenta pyramidar nem
emite um sinal de entrada conflitante enquanto acredita já estar
posicionada.

*Limitação reconhecida, não resolvida:* este rastreamento é uma suposição
otimista — `FeatureStrategy::on_features` nunca recebe a `RiskDecision`
real (`Signal` é fire-and-forget), então se o motor de risco rejeitar uma
entrada (saldo insuficiente, `PositionAlreadyOpen` por outra estratégia no
mesmo instrumento, ...), a estratégia ainda se comporta como se estivesse
posicionada até sua própria condição de saída disparar. Resolver isso de
verdade exigiria um canal de retorno de `RiskDecision` para
`FeatureStrategy` — fora do escopo desta auditoria (não pedido, e mudaria
o trait `FeatureStrategy`/`FeatureStrategyAdapter` do ADR-15). O sintoma
é limitado e autocorrigível: o pior caso é a estratégia ficar um tempo sem
avaliar novas entradas até sua condição de saída disparar naturalmente
(o `Flat` correspondente, se a posição na verdade não existir, é
rejeitado como `RejectionReason::NothingToFlatten` pelo motor de risco,
sem efeito colateral).

*Dois bugs reais encontrados e corrigidos ao escrever os testes de saída.*
Em `StatisticalMeanReversionStrategy` e `QuantMomentumStrategy`, a
implementação inicial usava `snapshot.zscore?`/`snapshot.regression?`
também no caminho de saída — mas `zscore` fica `None` quando a janela tem
desvio padrão zero, e `regression` fica `None` quando `SS_tot == 0` (ver
`features::ohlcv::zscore`/`regression`), ambos exatamente o caso em que o
preço ficou completamente parado dentro da janela — o resultado *mais*
decisivo de reversão/enfraquecimento possível, não "sem dado o bastante".
O `?` fazia a checagem de saída abortar silenciosamente nesse caso,
prendendo a estratégia numa posição que deveria ter sido fechada. Corrigido
tratando esse `None` explicitamente como "já revertido"/"tendência já
morreu" no caminho de saída (nunca no caminho de entrada, onde `None`
continua significando "sem dado suficiente para decidir direção"),
capturado pelos testes `emits_its_own_flat_exit_once_price_reverts_to_the_mean`
e `emits_its_own_flat_exit_once_the_trend_weakens`.

**(3) Confiança não-probabilística — verificado, reforçado com teste.**
`grep` em `crates/risk/src`, `crates/portfolio/src` e `crates/execution/src`
não encontra nenhuma referência a `confidence` — o dimensionamento de
posição em `RiskEngine::evaluate_entry` usa só `RiskConfig::order_notional`
(fixo), independentemente do sinal. Isso já era verdade antes desta
auditoria; formalizado com
`risk::engine::tests::signal_confidence_does_not_affect_position_sizing`,
que avalia dois sinais idênticos exceto pela confiança (`0.05` vs. `0.99`)
e confirma que a ordem aprovada tem exatamente a mesma quantidade nos dois
casos — se o dimensionamento algum dia passar a ler `confidence`, este
teste falha e sinaliza a mudança explicitamente. Cada uma das três
estratégias novas também documenta, no próprio arquivo, que sua saída
sempre reporta confiança `1.0` — não "100% de certeza de lucro", só "a
regra de saída, binária por natureza, disparou".

## ADR-18: Timing realista do backtest — sinal em close(T), fill em open(T+1)

**Contexto.** `BacktestRunner::run` submetia a ordem resultante de um sinal
na mesma iteração de candle que o produziu, usando `candle.close` como
preço de referência do fill — ou seja, um sinal calculado a partir do
fechamento do candle T era executado *no próprio* close(T). Isso é
otimista: em produção, o close de um candle só é conhecido no instante em
que ele fecha, e o próximo preço realisticamente executável é o do próximo
candle, não um preço já "passado" no momento da decisão. `app::pipeline::run`
(o loop live) preserva esse mesmo comportamento deliberadamente — não há,
ao vivo, um "próximo candle em lote" para esperar; o candle que acabou de
fechar É o instante de decisão e execução.

**Decisão.** `BacktestRunner::run` agora separa sinal e execução em duas
fases, com uma fila de ordens pendentes por instrumento
(`HashMap<InstrumentId, Vec<OrderRequest>>`, local ao método):

1. Ao processar um candle, primeiro drena qualquer ordem pendente para
   *aquele instrumento* (enfileirada no candle anterior daquele mesmo
   instrumento) e a executa usando `candle.open` como preço de referência e
   `candle.open_time` como instante de execução.
2. Só então avalia os sinais que este candle produz. Uma ordem aprovada
   pelo `RiskEngine` não é submetida ao `Broker` — é enfileirada para ser
   drenada no passo 1 da *próxima* vez que um candle deste instrumento for
   processado.
3. Se o instrumento nunca mais aparecer na reprodução (a ordem foi gerada
   pelo último candle da série), a ordem permanece pendente para sempre e é
   descartada ao final — nunca executada. Registrado via `tracing::info!`
   quando isso acontece.

A fila é por-instrumento porque a reprodução intercala candles de
múltiplos instrumentos em ordem cronológica global — "o próximo candle"
precisa significar "o próximo candle *daquele* instrumento", não
literalmente a próxima iteração do laço.

O dimensionamento/aprovação (`RiskEngine::evaluate`) continua avaliado com
o `close(T)` como preço de referência — "tamanho decidido no instante da
decisão" continua realista; só o preenchimento em si (via `Broker::
submit_order`, antes de spread/slippage/fees) é adiado para `open(T+1)`.

Saídas disparadas por risco (stop loss/take profit,
`check_risk_driven_exit` → renomeado `queue_risk_driven_exit`) recebem o
mesmo tratamento, por consistência: a saída é *decidida* no candle que
rompe o limiar, mas só *enfileirada*, executando no open() do candle
seguinte — mesma lógica de sinais de estratégia, sem um caminho especial.

`execution::Broker::submit_order` ganhou um parâmetro `execution_time:
DateTime<Utc>`, separado de `request.requested_at` (o instante do sinal).
Uma implementação usa `requested_at` para `Order::created_at` e
`execution_time` para `Fill::executed_at`/`Order::updated_at`/os
timestamps de posição — os dois podem coincidir (caminho live, que passa
sempre `candle.close_time` para os dois) ou divergir (backtest, que passa
o `open_time` do candle seguinte). `app::pipeline::run` foi adaptado
mecanicamente à nova assinatura, sem nenhuma mudança de comportamento.

**Testes.** `backtest::runner::tests`: `entry_order_fills_using_open_of_next_candle_as_reference_price`
(preço de referência é o open do próximo candle, não o preço do candle do
sinal), `entry_order_never_executes_when_no_next_candle_exists` (ordem
pendente sem próximo candle nunca executa), `risk_driven_exit_also_fills_using_open_of_next_candle`
(mesma deferência para saídas de risco), e o teste pré-existente
`signal_execution_never_precedes_the_candle_that_produced_it` foi
atualizado para provar a nova semântica de três candles (sem o gatilho,
sem posição; com o gatilho mas sem o próximo candle, ainda sem posição;
com os dois, posição com `opened_at` no open do candle seguinte).
`execution::paper_broker::tests::order_created_at_and_fill_executed_at_come_from_different_timestamps`
prova a separação dos dois timestamps na camada de `Broker`.

## ADR-19: `PositionQuery` — elimina o rastreamento otimista de posição nas estratégias

**Contexto.** ADR-17 documentou, como limitação reconhecida e não
resolvida, que as três estratégias quantitativas (`StatisticalMeanReversionStrategy`,
`QuantMomentumStrategy`, `VolatilityBreakoutStrategy`) rastreavam sua
própria posição (`position_side: HashMap<InstrumentId, Side>`) marcada
otimisticamente ao emitir um sinal de entrada — sem nunca saber se o
`RiskEngine` de fato aprovou aquele sinal. Se o motor de risco rejeitasse a
entrada (saldo insuficiente, `PositionAlreadyOpen` por outra estratégia no
mesmo instrumento, limite de exposição, ...), a estratégia continuava
acreditando estar posicionada — uma posição fantasma — e ficava presa
monitorando a saída de algo que nunca existiu, incapaz de reavaliar uma
entrada válida até sua própria condição de saída disparar "no vazio".

**Decisão.** Nova peça mínima, `strategies::PositionQuery`
(`crates/strategies/src/position_query.rs`):

```rust
pub trait PositionQuery {
    fn open_position_side(&self, instrument_id: InstrumentId) -> Option<Side>;
}
```

`Strategy::on_event` e `FeatureStrategy::on_features` ganharam um novo
parâmetro, `positions: &dyn PositionQuery`, repassado sem alteração por
`StrategyRegistry::dispatch` e por `FeatureStrategyAdapter`. As três
estratégias quantitativas perderam completamente o campo `position_side` —
não guardam mais nenhum estado de posição próprio. Onde antes liam
`self.position_side.get(&instrument.id)`, agora consultam
`positions.open_position_side(instrument.id)` a cada chamada, e onde antes
escreviam `.insert(...)`/`.remove(...)`, agora não escrevem nada — a
realidade é sempre consultada ao vivo, nunca cacheada.

A única implementação real é `impl PositionQuery for portfolio::PortfolioManager`
(`open_position_for(id).map(|p| p.side)`), no próprio crate `strategies`
(que passou a depender de `portfolio` para isso — dependência sancionada
deliberadamente, já que `PortfolioManager` é o único dono real do estado de
posição executado; `execution::Broker` e `persistence` continuam fora do
alcance de qualquer estratégia). `backtest::BacktestRunner::run` e
`app::pipeline::run` passam `&*portfolio` (o `PortfolioManager` real,
somente leitura) em cada `dispatch`. Isso satisfaz a exigência de não
duplicar estado financeiro: nenhum saldo, PnL ou preço médio é copiado para
dentro da estratégia — só a existência/lado de uma posição é lida, e nunca
armazenada.

Duas implementações adicionais, só para teste: `impl PositionQuery for
HashMap<InstrumentId, Side>` (simula posição real, inclusive uma rejeição
nunca refletida no mapa) e `NoPositions` (nunca reporta posição alguma,
para estratégias/testes que não dependem disso — as três baselines
`EmaCrossoverStrategy`/`MomentumStrategy`/`MeanReversionStrategy` nunca
rastrearam posição e continuam recebendo o parâmetro só para uniformidade
do trait, ignorando-o).

**Testes.** Cada uma das três estratégias quantitativas ganhou
`rejected_entry_leaves_the_strategy_able_to_detect_a_new_valid_entry_later`:
alimenta uma sequência de preços que satisfaz a condição de entrada mais de
uma vez, com um `positions` que nunca é atualizado (simulando rejeição
contínua do Risk Engine), e confirma que a estratégia emite o sinal de
entrada novamente, em vez de ficar presa. Os `run()` helpers de teste
pré-existentes (lógica de entrada/saída pura) foram adaptados para
sincronizar um `HashMap<InstrumentId, Side>` local a cada sinal emitido —
simulando o `PortfolioManager` real sendo atualizado após cada fill
aprovado — para que continuem provando exatamente o que provavam antes
(saída própria funciona depois de uma entrada de fato "executada").
`strategies::registry::tests::feature_strategy_dispatches_through_the_same_registry_as_baselines`
foi adaptado da mesma forma, provando o padrão de integração real através
do `StrategyRegistry`.

## ADR-20: "Posição aberta" é uma propriedade de (instrumento, robô), não só do instrumento

**Contexto.** A Fase 1.5 (`strategies::instance`) permitiu registrar
múltiplas instâncias de estratégia — "robôs" — inclusive mais de uma no
mesmo instrumento (`StrategyInstanceConfig { id, kind, symbols }`, id
arbitrário por instância). Mas `PortfolioManager`/`RiskEngine` continuavam
tratando "a posição aberta" como algo indexado só por `InstrumentId`:
`PortfolioManager::open_position_for(instrument_id)`,
`close_position(instrument_id, ...)`, `risk::PortfolioState::open_position_for(instrument_id)`
e a checagem `PositionAlreadyOpen` do `RiskEngine` só sabiam responder "há
uma posição neste instrumento?", nunca "há uma posição *deste robô* neste
instrumento?". Na prática isso impedia dois robôs configurados para o
mesmo símbolo de manterem posições simultâneas — o segundo a entrar era
sempre rejeitado, mesmo sendo uma estratégia e um capital logicamente
independentes.

**Decisão.** Toda busca/fechamento de posição passa a usar a chave
composta `(InstrumentId, StrategyId)`, não um tipo novo — `Position`,
`OrderRequest` e `Order` já carregam `strategy_id` desde sempre, e
`StrategyId` já é a identidade estável de um robô desde a Fase 1.5. Não
foi criado nenhum `RobotId`: isso teria sido uma segunda identidade de
estratégia, exatamente o que as fases anteriores evitaram deliberadamente.
`PortfolioManager` continua guardando `Vec<Position>` (não virou
`HashMap`) — a escala é de dezenas de posições em paper trading e todo
consumo já era iteração, não lookup por chave em hot path; só os filtros
de busca passaram a checar os dois campos:

```rust
pub fn open_position_for(&self, instrument_id: InstrumentId, strategy_id: &StrategyId) -> Option<&Position>;
pub fn open_positions_for_instrument(&self, instrument_id: InstrumentId) -> Vec<&Position>; // todos os robôs
pub fn open_positions_for_strategy(&self, strategy_id: &StrategyId) -> Vec<&Position>; // um robô, todos os instrumentos
pub fn exposure_for_instrument(&self, instrument_id: InstrumentId) -> Money;
pub fn close_position(&mut self, instrument_id: InstrumentId, strategy_id: &StrategyId, ...) -> Result<Position, PortfolioError>;
```

`risk::PortfolioState::open_position_for` ganhou o mesmo segundo
parâmetro, e `RiskEngine::evaluate` passou a consultá-lo com
`&signal.strategy_id` em vez de só `instrument.id` — `PositionAlreadyOpen`
agora significa "este robô já tem uma posição aqui", não "alguém tem".
`total_exposure`/`open_position_count`/`cash` (o caixa é um único saldo
compartilhado) permaneceram exatamente como estavam: já somavam sobre
todas as posições abertas, de todos os robôs, então o limite de
capital/exposição *global* — "um robô não pode gastar capital já
comprometido por outro" — já era garantido pela própria arquitetura
existente, sem nenhuma mudança de código, só testes novos que provam isso
no cenário multi-robô.

`strategies::PositionQuery` (o trait que as 6 estratégias consultam) **não
mudou** — continua `fn open_position_side(&self, instrument_id: InstrumentId) -> Option<Side>`,
de propósito: nenhuma estratégia precisa saber que outros robôs existem.
O que muda é quem responde. Uma peça nova e pequena,
`strategies::position_query::RobotPositions`
(`fn open_position_side_for(&self, instrument_id, strategy_id) -> Option<Side>`),
é a fonte multi-robô de verdade, implementada sobre `PortfolioManager`.
`StrategyRegistry::dispatch` passou a receber `positions: &dyn RobotPositions`
(era `&dyn PositionQuery`) e constrói, para cada entrada despachada, um
`ForStrategy` — um adaptador privado que implementa `PositionQuery`
filtrando por `entry.strategy.id()` — antes de chamar `Strategy::on_event`.
Nenhuma das 6 estratégias mudou uma linha: elas continuam vendo o mesmo
`&dyn PositionQuery` de sempre, só que agora corretamente escopado ao
próprio robô por construção, não por disciplina de quem chama.

**Consequências práticas.**
- `execution::PaperBroker::submit_order` decide abrir/fechar/rejeitar
  (naked short, same-side) com base na posição *deste* `request.strategy_id`,
  não em qualquer posição do instrumento — dois robôs podem ter ordens de
  lados opostos no mesmo instrumento na mesma passagem sem colidir.
- `app::pipeline::handle_risk_driven_exit` e
  `backtest::runner::queue_risk_driven_exit` (checagem de stop/take-profit
  por candle) passaram de "no máximo uma posição por instrumento" para um
  laço sobre `open_positions_for_instrument` — cada robô tem seu próprio
  `entry_price` e portanto seu próprio limiar de saída.
- O dedupe de `backtest::runner` ("não enfileirar a entrada de um sinal se
  já há uma saída por risco pendente para este instrumento no mesmo
  candle") passou a ser escopado por `(instrument_id, strategy_id)` — uma
  saída pendente do robô A não pode mais descartar, por engano, a entrada
  do robô B no mesmo instrumento.
- `PortfolioManager::restore` passou a rejeitar duplicidade por
  `(instrument_id, strategy_id)`, não por `instrument_id` sozinho — um
  restart com posições legítimas de dois robôs no mesmo instrumento não é
  mais tratado como uma inconsistência.
- `persistence`, `analytics` e `web` não mudaram: a tabela `positions`
  nunca teve uma constraint `UNIQUE` em `instrument_id` (só índices), e
  ambos os crates já agregavam PnL/exposição a partir de `Vec<Position>`
  plano, já com `strategy_id` — múltiplas posições simultâneas no mesmo
  instrumento já fluíam corretamente por eles.

**Limitação estrutural que permanece, fora deste escopo.** O
dimensionamento de posição (`RiskConfig::order_notional`) e os limites de
risco (`max_total_exposure`, `max_open_positions`, `max_daily_loss`)
continuam sendo *globais* ao portfólio inteiro, não configuráveis por
robô — um robô "ganancioso" pode consumir o caixa/exposição disponível
para os demais, dentro do mesmo `RiskEngine`/`RiskConfig` compartilhado.
Isso é uma decisão de produto (limites por robô vs. globais), não uma
limitação técnica desta mudança, e fica para quando o Strategy
Selector/Judge precisar dela.
