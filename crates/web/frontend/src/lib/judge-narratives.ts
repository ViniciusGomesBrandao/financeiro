import { fmtMoney, fmtPct, num } from '@/lib/format'
import { strategyGuide } from '@/lib/strategy-guides'
import type { JudgeDecisionJson, OperationalRobot } from '@/api/types'

export function robotHeadline(robot: OperationalRobot): string {
  const status =
    robot.status === 'running'
      ? 'está operando em paper trading'
      : 'está parado (não abre posições novas)'
  const pnl = num(robot.net_pnl) ?? 0
  const pnlWord =
    pnl > 0
      ? `ganhando ${fmtMoney(robot.net_pnl)}`
      : pnl < 0
        ? `perdendo ${fmtMoney(robot.net_pnl)}`
        : 'no zero'
  const trades =
    robot.closed_trades_count === 0
      ? 'ainda sem trades fechados'
      : `${robot.closed_trades_count} trade${robot.closed_trades_count === 1 ? '' : 's'} fechado${robot.closed_trades_count === 1 ? '' : 's'}`

  if (robot.active_strategy_id) {
    const kind = strategyKindFromInstance(robot.active_strategy_id)
    const guide = strategyGuide(kind)
    return `${robot.name} ${status} em ${robot.symbol}. O Judge escolheu ${guide.title} agora — ${pnlWord}, ${trades}.`
  }

  return `${robot.name} ${status} em ${robot.symbol}. O Judge ainda não definiu uma estratégia vencedora — ${pnlWord}, ${trades}.`
}

export function regimeLabel(regime: string | null | undefined): string {
  switch (regime) {
    case 'trending':
      return 'Tendência'
    case 'ranging':
      return 'Lateral'
    case 'volatility_expansion':
      return 'Expansão / rompimento'
    case 'uncertain':
      return 'Incerto'
    default:
      return regime ?? '—'
  }
}

export function judgeMoodLabel(mood: string): string {
  switch (mood) {
    case 'satisfied':
      return 'Satisfeito com a escolha'
    case 'looking':
      return 'Procurando / avaliando'
    case 'idle':
      return 'Aguardando dados'
    default:
      return mood
  }
}

export function judgeMoodSentence(robot: OperationalRobot): string {
  switch (robot.judge_mood) {
    case 'satisfied':
      return 'O Judge está satisfeito com a estratégia ativa neste momento.'
    case 'looking':
      return 'O Judge ainda está avaliando ou procurando uma opção melhor.'
    case 'idle':
      return 'Ainda não há avaliação do Judge para este robô.'
    default:
      return robot.active_why
  }
}

export function strategyKindFromInstance(instanceId: string): string {
  const parts = instanceId.split('::')
  return parts.length > 1 ? parts[parts.length - 1]! : instanceId
}

export function judgeStateLabel(state: string): string {
  switch (state) {
    case 'active':
      return 'Apta'
    case 'degraded':
      return 'Atenção'
    case 'disabled':
      return 'Não recomendada'
    default:
      return state
  }
}

export function judgeReasonSentence(reason: JudgeDecisionJson['reason']): string {
  switch (reason.kind) {
    case 'insufficient_sample':
      return `Ainda faltam dados: ${reason.trades} de ${reason.required} trades fechados para o Judge opinar com segurança.`
    case 'meets_all_thresholds':
      return 'Passou em todos os critérios econômicos configurados.'
    case 'below_consistency_threshold':
      return `Taxa de acerto (${fmtPct(reason.win_rate)}) abaixo do mínimo (${fmtPct(reason.min)}).`
    case 'below_profit_factor_threshold':
      return `Profit factor (${reason.profit_factor}) abaixo do mínimo (${reason.min}).`
    case 'non_positive_expectancy':
      return `Em média, cada trade desta estratégia não está ganhando dinheiro (expectancy ${reason.expectancy}).`
    case 'excessive_drawdown':
      return `Drawdown (${reason.drawdown}) ultrapassa o limite (${reason.max}).`
    case 'transition_pending':
      return `Mudança de estado sugerida (${judgeStateLabel(reason.proposed ?? '')}), aguardando confirmação (${reason.confirmations}/${reason.required}).`
    default:
      return 'Motivo registrado pelo Judge.'
  }
}

export function expectancySentence(expectancy: string): string {
  const n = num(expectancy)
  if (n === null) return 'Expectancy indisponível.'
  if (n > 0)
    return `Em média, cada operação desta estratégia está ganhando cerca de ${fmtPct(expectancy)}.`
  if (n < 0)
    return `Em média, cada operação desta estratégia está perdendo cerca de ${fmtPct(expectancy)}.`
  return 'Em média, cada operação desta estratégia está no zero.'
}

export function switchNarrative(
  symbol: string,
  previous: string | null,
  next: string | null,
): string {
  const prev = previous ? strategyGuide(strategyKindFromInstance(previous)).title : 'nenhuma'
  const neu = next ? strategyGuide(strategyKindFromInstance(next)).title : 'nenhuma'
  return `Em ${symbol}, o robô trocou de ${prev} para ${neu}.`
}
