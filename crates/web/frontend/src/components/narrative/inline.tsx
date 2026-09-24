import type { ReactNode } from 'react'
import { MoneyNumber } from '@/components/animate-ui/sliding-number'
import { Tip } from '@/components/tip'
import { num, signTone } from '@/lib/format'
import { cn } from '@/lib/utils'

export function PlainSentence({
  children,
  className,
}: {
  children: ReactNode
  className?: string
}) {
  return (
    <p className={cn('text-[15px] leading-snug text-foreground/95', className)}>{children}</p>
  )
}

export function InlineAsset({
  symbol,
  tip,
}: {
  symbol: string
  tip?: string
}) {
  return (
    <span className="inline-flex items-baseline gap-0.5 font-semibold text-foreground">
      {symbol}
      {tip ? <Tip text={tip} /> : null}
    </span>
  )
}

export function InlineStrategy({
  id,
  tip,
}: {
  id: string
  tip?: string
}) {
  return (
    <span className="inline-flex items-baseline gap-0.5 font-medium text-accent">
      {id}
      {tip ? <Tip text={tip} /> : null}
    </span>
  )
}

export function InlineMoney({
  value,
  tip,
  signed,
  animate,
}: {
  value: string | number | null | undefined
  tip?: string
  signed?: boolean
  animate?: boolean
}) {
  const n = num(value)
  const tone = signed ? signTone(n) : 'neutral'
  return (
    <span
      className={cn(
        'inline-flex items-baseline gap-0.5 font-mono text-[0.95em] font-semibold tabular-nums',
        tone === 'positive' && 'text-positive',
        tone === 'negative' && 'text-negative',
      )}
    >
      {animate ? <MoneyNumber value={n} className="text-[1em]" /> : n === null ? '—' : formatInline(n)}
      {tip ? <Tip text={tip} /> : null}
    </span>
  )
}

function formatInline(n: number): string {
  return n.toLocaleString('pt-BR', { minimumFractionDigits: 2, maximumFractionDigits: 2 })
}

export function InlineTime({ iso }: { iso: string }) {
  return (
    <time dateTime={iso} className="whitespace-nowrap text-muted-foreground">
      {new Date(iso).toLocaleString('pt-BR')}
    </time>
  )
}

export function SentenceBreak() {
  return <span className="text-muted-foreground"> · </span>
}
