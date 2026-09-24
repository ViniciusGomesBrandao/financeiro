/** Textos de ajuda preservados/adaptados da UI anterior — linguagem leiga. */
export const TIPS = {
  paperTrading:
    'Paper trading é uma simulação: o robô toma decisões e "executa ordens" com dados de mercado reais, mas usando dinheiro fictício controlado só neste sistema. Nenhuma ordem é enviada a uma corretora de verdade, nenhum valor real é movimentado.',
  cash: 'Quanto dinheiro (simulado) está livre, sem estar preso em nenhuma posição aberta no momento.',
  equity:
    'O valor total simulado da conta agora: saldo em caixa + o valor de mercado atual das posições abertas. É o número que resume "quanto vale a carteira" neste instante.',
  pnlToday:
    'Lucro ou prejuízo já realizado (de trades fechados) hoje. Só conta o que já foi encerrado, não posições ainda abertas. Positivo é bom, negativo é ruim.',
  pnlTotal:
    'Lucro ou prejuízo acumulado de todos os trades já fechados desde o início. Positivo é bom, negativo é ruim.',
  unrealized:
    'Lucro ou prejuízo "no papel" das posições que ainda estão abertas, calculado com o preço atual de mercado — pode subir ou descer até a posição ser fechada. Positivo é bom, negativo é ruim.',
  returnPct:
    'Variação percentual do patrimônio desde o início, comparado ao capital inicial. Positivo é bom, negativo é ruim.',
  maxDrawdown:
    'A maior queda que o patrimônio já sofreu do topo até o fundo. Mede o "pior momento" — quanto menor (mais perto de zero), melhor, pois indica menos risco de perdas profundas.',
  openCount: 'Quantas posições (compras ainda não fechadas) o robô mantém em aberto agora.',
  exposure:
    'Que fração do patrimônio total está hoje investida em posições abertas, em vez de parada em caixa. Mais alto significa mais capital em risco no mercado agora; mais baixo significa mais capital parado e protegido.',
  exposedCapital:
    'Quanto dinheiro (simulado) está hoje comprometido em posições abertas, em vez de parado em caixa.',
  openPositionsNow:
    'Quantas compras o robô fez e ainda não fechou. Cada uma é uma aposta em aberto: só vira lucro ou prejuízo real quando for fechada.',
  price:
    'Último preço de mercado conhecido para este ativo, recebido em tempo real da Binance. Não é necessariamente o preço da última operação do robô.',
  side:
    'Se o robô comprou (compra aberta, lucra se o preço subir). Neste sistema spot, posição aberta é sempre compra — nunca abre venda a descoberto.',
  sideTrade:
    'Ciclo da operação: a posição era uma compra. "Fechamento" significa que a compra foi encerrada (ordem Sell de saída), não que o robô abriu uma venda a descoberto.',
  duration: 'Há quanto tempo esta posição está aberta, desde a compra até agora.',
  trades:
    'Quantidade de operações já fechadas (entrada + saída) por esta estratégia. Só conta o que já foi encerrado, não posições ainda abertas.',
  winners: 'Quantos desses trades fechados deram lucro (P&L líquido positivo).',
  losers: 'Quantos desses trades fechados deram prejuízo (P&L líquido negativo).',
  winRate:
    'Percentual de trades fechados que deram lucro (ganhos / total de trades). Mais alto é melhor, mas não conta a história toda: uma estratégia pode ganhar poucas vezes só que muito, e ainda ser lucrativa — olhe também o P&L líquido e o profit factor.',
  grossPnl:
    'Lucro ou prejuízo somado de todos os trades fechados, antes de descontar custos (fees, spread, slippage). Positivo é bom, negativo é ruim.',
  netPnl:
    'O que sobra do P&L bruto depois de descontar fees, spread e slippage — é o resultado real da estratégia. É o número mais importante para julgar desempenho. Positivo é bom, negativo é ruim.',
  avgWin: 'Valor médio dos trades que deram lucro. Quanto maior, melhor.',
  avgLoss: 'Valor médio dos trades que deram prejuízo. Quanto menor (mais perto de zero), melhor.',
  profitFactor:
    'Soma dos ganhos dividida pela soma das perdas. Acima de 1 significa que os ganhos superam as perdas no total; abaixo de 1 significa prejuízo no total. Quanto maior acima de 1, melhor.',
  tradeGross:
    'Lucro ou prejuízo deste trade antes de descontar custos (fees, spread, slippage).',
  fees: 'Taxa cobrada pela corretora/exchange para executar a ordem. É sempre um custo — quanto menor, melhor.',
  spread:
    'Diferença entre o preço de compra e o de venda no mercado no momento da ordem — um custo embutido de negociar, cobrado mesmo em uma execução "perfeita". Quanto menor, melhor.',
  slippage:
    'Diferença entre o preço esperado e o preço realmente obtido na execução, geralmente por o mercado se mover no instante da ordem. Quanto menor, melhor.',
  tradeNet:
    'O resultado real deste trade: bruto menos fees, spread e slippage. Positivo é bom, negativo é ruim.',
  approved:
    'O motor de risco aceitou este sinal e ele virou uma ordem de verdade (executada em papel).',
  rejected:
    'O motor de risco recusou este sinal — nenhuma ordem foi criada. É um comportamento normal e esperado quando as regras de risco bloqueiam a entrada, não um erro do sistema.',
  trigger:
    'O que disparou esta decisão: um sinal novo da estratégia, ou o monitoramento de risco fechando a posição sozinho por stop loss (limite de perda) ou take profit (meta de lucro).',
  confidence:
    'Não é uma probabilidade estatística de lucro nem um resultado de backtest — é apenas quão forte o sinal técnico da própria estratégia está, numa escala de 0% a 100% (ex.: o quanto o preço se afastou da média, ou a força da tendência medida pela estratégia). Um sinal com confiança alta pode dar prejuízo, e um com confiança baixa pode dar lucro; use como indicador de força do sinal, não como garantia.',
  panelPositions:
    'Compras que o robô ainda não fechou. Cada card é uma aposta em aberto: o P&L aqui ainda pode mudar até a venda.',
  panelPositionCard:
    'Esta linha é uma posição aberta neste ativo com esta estratégia. O valor colorido é o P&L não realizado (no papel).',
  panelLastAction:
    'A decisão mais recente do robô: se tentou entrar, se o risco aprovou ou bloqueou, e por quê. Não é um trade completo — é só o último “sim/não” do motor.',
  panelClosedTrades:
    'Histórico de operações já encerradas (compra + venda). O número à direita é o P&L líquido daquele trade, depois de fees, spread e slippage.',
  panelClosedTradeRow:
    'Um trade completo já fechado. Mostra ativo, estratégia, horário do fechamento e o resultado líquido (positivo = lucro, negativo = prejuízo).',
  panelStrategies:
    'Como cada estratégia está se saindo no total dos trades já fechados. A barra compara o tamanho do P&L líquido entre elas.',
  panelStrategyCard:
    'Resumo desta estratégia: P&L líquido acumulado, quantidade de trades e taxa de acerto. A barra só compara magnitude entre estratégias — não é progresso de meta.',
  panelActivity:
    'Fila de decisões recentes do robô (aprovadas ou bloqueadas pelo risco). “ok” = virou ordem em papel; “block” = o risco impediu.',
  panelActivityRow:
    'Uma decisão individual: ativo, estratégia, se passou no risco, e o motivo ou tipo de sinal.',
  panelMarket:
    'Últimos preços de mercado que o sistema está recebendo da exchange (referência, não necessariamente o preço da sua última ordem).',
  strategyId:
    'Nome interno da estratégia que gerou o sinal (ex.: mean_reversion, momentum). Cada uma tem regras próprias de entrada/saída.',
} as const
