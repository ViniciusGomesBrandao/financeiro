export function num(v: string | number | null | undefined): number | null {
  if (v === null || v === undefined) return null
  const n = typeof v === 'number' ? v : Number(v)
  return Number.isFinite(n) ? n : null
}

export function fmtMoney(v: string | number | null | undefined): string {
  const n = num(v)
  if (n === null) return '—'
  return n.toLocaleString('pt-BR', { minimumFractionDigits: 2, maximumFractionDigits: 2 })
}

export function fmtQty(v: string | number | null | undefined): string {
  const n = num(v)
  if (n === null) return '—'
  return n.toLocaleString('pt-BR', { minimumFractionDigits: 0, maximumFractionDigits: 8 })
}

export function fmtPct(v: string | number | null | undefined): string {
  const n = num(v)
  if (n === null) return '—'
  return (
    (n * 100).toLocaleString('pt-BR', { minimumFractionDigits: 2, maximumFractionDigits: 2 }) + '%'
  )
}

export function fmtTime(iso: string | null | undefined): string {
  if (!iso) return '—'
  return new Date(iso).toLocaleString('pt-BR')
}

export function fmtDuration(iso: string | null | undefined): string {
  if (!iso) return '—'
  const ms = Date.now() - new Date(iso).getTime()
  if (ms < 0) return '0m'
  const totalMinutes = Math.floor(ms / 60000)
  const days = Math.floor(totalMinutes / 1440)
  const hours = Math.floor((totalMinutes % 1440) / 60)
  const minutes = totalMinutes % 60
  if (days > 0) return `${days}d ${hours}h`
  if (hours > 0) return `${hours}h ${minutes}m`
  return `${minutes}m`
}

export function signTone(
  v: string | number | null | undefined,
): 'positive' | 'negative' | 'neutral' {
  const n = num(v)
  if (n === null || n === 0) return 'neutral'
  return n > 0 ? 'positive' : 'negative'
}

export function sideLabel(side: string | null | undefined): string {
  if (!side) return '—'
  const s = side.toLowerCase()
  // Spot: Position.side / Trade.side refletem a direção da posição (Buy = compra
  // aberta). Sell numa ordem de saída não deve ser lido como "abriu venda".
  if (s === 'buy') return 'Compra'
  if (s === 'sell') return 'Fechamento'
  return side
}

export const DIRECTION_LABEL: Record<string, string> = {
  long: 'compra (long)',
  short: 'venda (short)',
  flat: 'zerar posição',
}

export const TRIGGER_LABEL: Record<string, string> = {
  signal: 'sinal de estratégia',
  stop_loss: 'stop loss',
  take_profit: 'take profit',
}

export const SIDE_LABEL: Record<string, string> = {
  buy: 'compra',
  sell: 'fechamento',
}
