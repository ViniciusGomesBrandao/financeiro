import type { ReactNode } from 'react'
import type { OpenPosition } from '@/api/types'
import { Stagger, StaggerItem } from '@/components/animate-ui/motion'
import { HoverCard, HoverCardContent, HoverCardTrigger } from '@/components/animate-ui/hover-card'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Tip } from '@/components/tip'
import { fmtDuration, fmtMoney, fmtQty, fmtTime, num, sideLabel, signTone } from '@/lib/format'
import { TIPS } from '@/lib/tips'
import { cn } from '@/lib/utils'

export function PositionsSection({ positions }: { positions: OpenPosition[] }) {
  return (
    <section className="space-y-2.5">
      <div className="flex items-end justify-between gap-3">
        <div>
          <h2 className="text-base font-semibold tracking-tight">Posições abertas</h2>
          <p className="text-xs text-muted-foreground">Hover no card para detalhe</p>
        </div>
        <Badge tone="neutral">{positions.length}</Badge>
      </div>

      {positions.length === 0 ? (
        <Card>
          <CardContent className="p-8 text-center text-sm text-muted-foreground">
            Nenhuma posição aberta no momento.
          </CardContent>
        </Card>
      ) : (
        <Stagger className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {positions.map((p) => (
            <StaggerItem key={`${p.symbol}-${p.strategy_id}-${p.opened_at}`}>
              <PositionCard position={p} />
            </StaggerItem>
          ))}
        </Stagger>
      )}
    </section>
  )
}

function PositionCard({ position }: { position: OpenPosition }) {
  const tone = signTone(position.unrealized_pnl)
  const entry = num(position.entry_price) ?? 0
  const current = num(position.current_price) ?? entry
  const deltaPct = entry !== 0 ? ((current - entry) / entry) * 100 : 0

  return (
    <HoverCard>
      <HoverCardTrigger asChild>
        <Card className="cursor-default overflow-hidden transition-colors hover:border-accent/45">
          <CardHeader className="flex-row items-start justify-between gap-3 space-y-0">
            <div>
              <CardTitle>{position.symbol}</CardTitle>
              <p className="text-sm text-accent">{position.strategy_id}</p>
            </div>
            <Badge
              tone={tone === 'positive' ? 'positive' : tone === 'negative' ? 'negative' : 'neutral'}
            >
              {fmtMoney(position.unrealized_pnl)}
            </Badge>
          </CardHeader>
          <CardContent className="space-y-3">
            <div className="grid grid-cols-2 gap-2 text-sm">
              <Field
                label={
                  <span className="inline-flex items-center gap-1">
                    Lado <Tip text={TIPS.side} />
                  </span>
                }
                value={sideLabel(position.side)}
              />
              <Field
                label={
                  <span className="inline-flex items-center gap-1">
                    Duração <Tip text={TIPS.duration} />
                  </span>
                }
                value={fmtDuration(position.opened_at)}
              />
              <Field label="Entrada" value={fmtMoney(position.entry_price)} mono />
              <Field
                label="Atual"
                value={position.current_price != null ? fmtMoney(position.current_price) : '—'}
                mono
              />
            </div>
            <div>
              <div className="mb-1 flex justify-between text-[11px] text-muted-foreground">
                <span>Variação</span>
                <span
                  className={cn(
                    'font-mono',
                    deltaPct > 0 && 'text-positive',
                    deltaPct < 0 && 'text-negative',
                  )}
                >
                  {deltaPct.toLocaleString('pt-BR', { maximumFractionDigits: 2 })}%
                </span>
              </div>
              <div className="h-1.5 overflow-hidden rounded-full bg-muted">
                <div
                  className={cn(
                    'h-full rounded-full transition-all duration-700',
                    deltaPct >= 0 ? 'bg-positive' : 'bg-negative',
                  )}
                  style={{ width: `${Math.min(100, Math.abs(deltaPct) * 8 + 8)}%` }}
                />
              </div>
            </div>
          </CardContent>
        </Card>
      </HoverCardTrigger>
      <HoverCardContent side="top" align="center">
        <p className="mb-2 text-sm font-semibold">
          {position.symbol} · {position.strategy_id}
        </p>
        <div className="space-y-1.5 text-xs">
          <Row label="Quantidade" value={fmtQty(position.quantity)} />
          <Row label="Preço de entrada" value={fmtMoney(position.entry_price)} />
          <Row
            label="Preço atual"
            value={position.current_price != null ? fmtMoney(position.current_price) : '—'}
          />
          <Row
            label={
              <span className="inline-flex items-center gap-1">
                P&L <Tip text={TIPS.unrealized} />
              </span>
            }
            value={
              <span
                className={cn(
                  'font-mono',
                  tone === 'positive' && 'text-positive',
                  tone === 'negative' && 'text-negative',
                )}
              >
                {fmtMoney(position.unrealized_pnl)}
              </span>
            }
          />
          <Row label="Aberta em" value={fmtTime(position.opened_at)} />
        </div>
      </HoverCardContent>
    </HoverCard>
  )
}

function Field({
  label,
  value,
  mono,
}: {
  label: ReactNode
  value: ReactNode
  mono?: boolean
}) {
  return (
    <div className="rounded-lg bg-muted/35 px-3 py-2">
      <div className="text-[11px] text-muted-foreground">{label}</div>
      <div className={cn('mt-0.5 font-medium', mono && 'font-mono tabular-nums')}>{value}</div>
    </div>
  )
}

function Row({ label, value }: { label: ReactNode; value: ReactNode }) {
  return (
    <div className="flex items-start justify-between gap-4 border-b border-border/50 py-1.5 last:border-0">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="text-right font-medium">{value}</dd>
    </div>
  )
}
