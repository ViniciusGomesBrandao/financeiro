import type { OpenPosition, Overview, TimelineEntry } from '@/api/types'
import { SIDE_LABEL, fmtMoney, fmtQty, fmtTime, num } from '@/lib/format'

/** Dados estruturados para montar frases — zero cálculo financeiro novo. */

export type NowSummary = {
  watchingOnly: boolean
  openCount: number
  equity: number | null
  pnlToday: number | null
  exposed: number | null
  exposedPct: number | null
}

export function nowSummary(overview: Overview, positions: OpenPosition[]): NowSummary {
  const exposed = positions.reduce((sum, p) => {
    const price = num(p.current_price) ?? num(p.entry_price) ?? 0
    const qty = num(p.quantity) ?? 0
    return sum + price * qty
  }, 0)
  const equity = num(overview.equity)
  const exposedPct = equity && equity !== 0 ? (exposed / equity) * 100 : null
  const openCount = overview.open_positions_count ?? positions.length

  return {
    watchingOnly: openCount === 0,
    openCount,
    equity,
    pnlToday: num(overview.realized_pnl_today),
    exposed: openCount > 0 ? exposed : 0,
    exposedPct,
  }
}

export type OpenPositionNarrative = {
  symbol: string
  strategyId: string
  openedAt: string
  entryNotional: number | null
  unrealized: number | null
  quantity: string
  entryPrice: string
}

export function openPositionNarrative(p: OpenPosition): OpenPositionNarrative {
  const qty = num(p.quantity)
  const entry = num(p.entry_price)
  const entryNotional = qty !== null && entry !== null ? qty * entry : null
  return {
    symbol: p.symbol,
    strategyId: p.strategy_id,
    openedAt: p.opened_at,
    entryNotional,
    unrealized: num(p.unrealized_pnl),
    quantity: p.quantity,
    entryPrice: p.entry_price,
  }
}

/** Frase pura (texto) — útil para aria/testes; a UI usa a versão com micro-peças. */
export function openPositionSentenceText(p: OpenPosition): string {
  const n = openPositionNarrative(p)
  const when = fmtTime(n.openedAt)
  const value = n.entryNotional !== null ? fmtMoney(n.entryNotional) : '—'
  const pnl = n.unrealized !== null ? fmtMoney(n.unrealized) : '—'
  const verb =
    n.unrealized !== null && n.unrealized < 0
      ? 'está perdendo'
      : n.unrealized !== null && n.unrealized > 0
        ? 'está rendendo'
        : 'está empatado em'
  return `Comprou ${n.symbol} com a estratégia ${n.strategyId} em ${when}, no valor de ${value}. Até agora ${verb} ${pnl}.`
}

export type LastActionNarrative = {
  kind: 'none' | 'approved' | 'rejected'
  symbol: string
  strategyId: string
  when: string
  directionWord: string
  reason: string | null
  fillQty: string | null
  fillPrice: string | null
  sideWord: string | null
  pnlNet: number | null
}

export function lastActionNarrative(e: TimelineEntry | null): LastActionNarrative {
  if (!e) {
    return {
      kind: 'none',
      symbol: '',
      strategyId: '',
      when: '',
      directionWord: '',
      reason: null,
      fillQty: null,
      fillPrice: null,
      sideWord: null,
      pnlNet: null,
    }
  }

  const directionWord =
    e.signal_direction === 'long'
      ? 'compra'
      : e.signal_direction === 'short'
        ? 'fechamento'
        : e.signal_direction === 'flat'
          ? 'zerar posição'
          : 'ação'

  return {
    kind: e.approved ? 'approved' : 'rejected',
    symbol: e.symbol,
    strategyId: e.strategy_id,
    when: e.created_at,
    directionWord,
    reason: e.reason,
    fillQty: e.order_quantity,
    fillPrice: e.fill_price,
    sideWord: e.order_side ? SIDE_LABEL[e.order_side.toLowerCase()] ?? e.order_side : null,
    pnlNet: num(e.pnl_net),
  }
}

export function lastActionSentenceText(e: TimelineEntry | null): string {
  const a = lastActionNarrative(e)
  if (a.kind === 'none') return 'Nenhuma decisão registrada ainda.'
  if (a.kind === 'rejected') {
    const why = a.reason ? ` — ${a.reason}` : ''
    return `Tentou ${a.directionWord} de ${a.symbol} com ${a.strategyId}${why}. O risco bloqueou.`
  }
  let text = `Sinal de ${a.directionWord} em ${a.symbol} com a estratégia ${a.strategyId} foi aprovado`
  if (a.sideWord && a.fillQty) {
    text += ` e executou ${a.sideWord} de ${fmtQty(a.fillQty)}`
    if (a.fillPrice) text += ` a ${fmtMoney(a.fillPrice)}`
  }
  if (a.pnlNet !== null) text += ` (resultado ${fmtMoney(a.pnlNet)})`
  text += ` — ${fmtTime(a.when)}`
  return text
}

export function watchingSentenceText(summary: NowSummary): string {
  const equity = summary.equity !== null ? fmtMoney(summary.equity) : '—'
  return `O robô está só olhando o mercado. Nenhuma compra aberta. Patrimônio simulado: ${equity}.`
}
