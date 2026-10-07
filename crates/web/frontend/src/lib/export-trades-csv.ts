import type {
  JudgeEvaluation,
  OperationalRobot,
  RobotDetail,
  StrategySwitch,
  Trade,
} from '@/api/types'
import { DIRECTION_LABEL, TRIGGER_LABEL, fmtHold } from '@/lib/format'
import {
  judgeReasonSentence,
  regimeLabel,
  strategyKindFromInstance,
  switchNarrative,
} from '@/lib/judge-narratives'
import { strategyGuide } from '@/lib/strategy-guides'

const HEADERS = [
  'trade_id',
  'robot_id',
  'robot_name',
  'symbol',
  'timeframe',
  'strategy_id',
  'strategy',
  'direction',
  'entry_confidence',
  'quantity',
  'entry_price',
  'exit_price',
  'opened_at',
  'closed_at',
  'duration',
  'pnl_gross',
  'fees_paid',
  'spread_paid',
  'slippage_paid',
  'pnl_net',
  'exit_trigger',
  'exit_reason',
  'regime_at_entry',
  'regime_strength',
  'regime_summary',
  'selection_summary',
  'judge_selected_strategy',
  'judge_evaluated_at',
  'related_switch',
  'related_switch_reason',
  'related_switch_at',
] as const

function csvEscape(value: string | number | null | undefined): string {
  if (value === null || value === undefined) return ''
  const s = String(value)
  if (/[",\r\n]/.test(s)) return `"${s.replace(/"/g, '""')}"`
  return s
}

function tradeContext(
  trade: Trade,
  evaluations: JudgeEvaluation[],
  switches: StrategySwitch[],
) {
  const entryEval = [...evaluations]
    .sort((a, b) => new Date(b.evaluated_at).getTime() - new Date(a.evaluated_at).getTime())
    .find((e) => new Date(e.evaluated_at).getTime() <= new Date(trade.opened_at).getTime())

  const relatedSwitch = switches.find((s) => {
    const t = new Date(s.switched_at).getTime()
    const open = new Date(trade.opened_at).getTime()
    const close = new Date(trade.closed_at).getTime()
    return t >= open - 1000 && t <= close + 1000
  })

  const raw = entryEval?.decisions
  const payload =
    raw && !Array.isArray(raw)
      ? { regime: raw.regime, selection_reason: raw.selection_reason }
      : null

  return { entryEval, relatedSwitch, payload }
}

function rowForTrade(
  trade: Trade,
  robot: OperationalRobot,
  evaluations: JudgeEvaluation[],
  switches: StrategySwitch[],
): string[] {
  const ctx = tradeContext(trade, evaluations, switches)
  const strategyTitle = strategyGuide(strategyKindFromInstance(trade.strategy_id)).title
  const direction =
    trade.entry_direction ?? (trade.side.toLowerCase() === 'buy' ? 'long' : trade.side)
  const exitTrigger = trade.exit_trigger
    ? TRIGGER_LABEL[trade.exit_trigger] ?? trade.exit_trigger
    : ''

  return [
    trade.id ?? '',
    robot.id,
    robot.name,
    trade.symbol,
    robot.timeframe,
    trade.strategy_id,
    strategyTitle,
    DIRECTION_LABEL[direction] ?? direction,
    trade.entry_confidence != null ? String(trade.entry_confidence) : '',
    trade.quantity,
    trade.entry_price,
    trade.exit_price,
    trade.opened_at,
    trade.closed_at,
    fmtHold(trade.opened_at, trade.closed_at),
    trade.pnl_gross,
    trade.fees_paid,
    trade.spread_paid,
    trade.slippage_paid,
    trade.pnl_net,
    exitTrigger,
    trade.exit_reason ?? '',
    regimeLabel(ctx.payload?.regime?.kind ?? null),
    ctx.payload?.regime?.strength != null ? String(ctx.payload.regime.strength) : '',
    ctx.payload?.regime?.summary ?? '',
    ctx.payload?.selection_reason?.summary ?? '',
    ctx.entryEval?.selected_strategy_id
      ? strategyGuide(strategyKindFromInstance(ctx.entryEval.selected_strategy_id)).title
      : '',
    ctx.entryEval?.evaluated_at ?? '',
    ctx.relatedSwitch
      ? switchNarrative(
          trade.symbol,
          ctx.relatedSwitch.previous_strategy_id,
          ctx.relatedSwitch.new_strategy_id,
        )
      : '',
    ctx.relatedSwitch ? judgeReasonSentence(ctx.relatedSwitch.reason) : '',
    ctx.relatedSwitch?.switched_at ?? '',
  ]
}

/** Monta CSV das últimas transações do robô + contexto histórico do Judge. */
export function buildTradesCsv(
  robot: OperationalRobot,
  detail: RobotDetail,
  limit = 200,
): string {
  const trades = detail.trades.slice(0, limit)
  const lines = [
    HEADERS.join(','),
    ...trades.map((t) =>
      rowForTrade(t, robot, detail.evaluations, detail.switches).map(csvEscape).join(','),
    ),
  ]
  // BOM ajuda o Excel no Windows a reconhecer UTF-8
  return `\uFEFF${lines.join('\r\n')}\r\n`
}

export function downloadTradesCsv(
  robot: OperationalRobot,
  detail: RobotDetail,
  limit = 200,
): void {
  const csv = buildTradesCsv(robot, detail, limit)
  const stamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19)
  const filename = `quant-engine_${robot.id}_trades_${stamp}.csv`
  const blob = new Blob([csv], { type: 'text/csv;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  a.rel = 'noopener'
  document.body.appendChild(a)
  a.click()
  a.remove()
  URL.revokeObjectURL(url)
}
