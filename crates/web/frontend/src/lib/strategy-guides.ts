/** Explicações leigas das estratégias — espelha o racional dos crates Rust, sem inventar edge. */
export type StrategyGuide = {
  title: string
  idea: string
  how: string
  failsWhen: string
}

const GUIDES: Record<string, StrategyGuide> = {
  mean_reversion: {
    title: 'Reversão à média',
    idea: 'Quando o preço se afasta muito da média recente, a estratégia aposta que ele tende a voltar para perto dessa média — opera “contra o extremo”, não a favor da tendência.',
    how: 'Olha candles fechados, calcula média e desvio (z-score). Se o preço está longe demais da média (acima do limiar), emite sinal de compra ou venda conforme o lado do extremo.',
    failsWhen:
      'Mercados em tendência forte (em vez de lateral): remar contra a onda pode acumular prejuízo. Também sofre em mudanças bruscas de regime (notícias, listagens).',
  },
  momentum: {
    title: 'Momentum',
    idea: 'Se o preço subiu (ou caiu) com força nos últimos candles, a estratégia aposta que o movimento continua um pouco mais — segue a direção recente.',
    how: 'Mede o retorno ao longo de uma janela (lookback). Se passar de um limiar mínimo (filtro de ruído), emite sinal na direção do movimento.',
    failsWhen:
      'Reversões bruscas: ela “corre atrás” do movimento e entra atrasada. Janelas curtas demais confundem ruído com tendência.',
  },
  ema_crossover: {
    title: 'Cruzamento de EMAs',
    idea: 'Heurística clássica de tendência: uma média rápida cruzando uma média lenta sugere mudança no ritmo de curto prazo vs. o de mais longo prazo.',
    how: 'Mantém duas EMAs (rápida e lenta) nos fechamentos. Quando a rápida cruza a lenta para cima ou para baixo, emite sinal. A “confiança” é só a distância relativa entre as médias — não é probabilidade real.',
    failsWhen:
      'Mercados lateralizados geram cruzamentos falsos (whipsaws). É atrasada por construção: confirma a tendência depois que ela já começou.',
  },
  statistical_mean_reversion: {
    title: 'Reversão estatística',
    idea: 'Versão quantitativa da reversão à média: combina z-score, volatilidade e filtros de regime para só operar quando o extremo parece estatisticamente significativo.',
    how: 'Usa features calculadas sobre candles fechados (média, desvio, volatilidade). Só emite sinal quando vários filtros conjuntivos concordam que o preço está “longe demais” e há condições para voltar.',
    failsWhen:
      'Tendências fortes prolongadas e mudanças de regime (notícias, liquidez). Filtros conservadores podem ficar em silêncio por muito tempo.',
  },
  quant_momentum: {
    title: 'Momentum quantitativo',
    idea: 'Momentum com camadas extras: não basta o preço ter subido — volume, volatilidade e consistência do movimento precisam confirmar que a força é real.',
    how: 'Combina retorno recente com features de volume e volatilidade. Emite compra quando o conjunto indica continuação; pode emitir saída explícita (flat) quando a força some.',
    failsWhen:
      'Reversões rápidas após um pico de momentum (“bull trap”). Mercados muito calmos geram poucos sinais úteis.',
  },
  volatility_breakout: {
    title: 'Rompimento de volatilidade',
    idea: 'Aposta que um período de compressão (baixa volatilidade) costuma preceder um movimento direcional mais forte — entra quando o preço “rompe” a faixa recente.',
    how: 'Mede a faixa de preços/volatilidade em uma janela. Quando o fechamento rompe o limite superior (ou inferior) com confirmação dos filtros, emite sinal na direção do rompimento.',
    failsWhen:
      'Falsos rompimentos em mercados laterais (o preço volta para dentro da faixa). Notícias podem invalidar o padrão imediatamente após a entrada.',
  },
}

export function strategyGuide(id: string): StrategyGuide {
  return (
    GUIDES[id] ?? {
      title: id,
      idea: 'Estratégia registrada no sistema. O nome identifica a regra que gerou o sinal; os números à direita são o resultado dos trades já fechados com ela.',
      how: 'Consulte a documentação do crate de strategies para os parâmetros exatos desta regra.',
      failsWhen: 'Depende do regime de mercado e dos parâmetros configurados.',
    }
  )
}
