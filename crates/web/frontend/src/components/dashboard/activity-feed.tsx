import { Expand } from 'lucide-react'
import type { ReactNode } from 'react'
import type { Price, TimelineEntry, Trade } from '@/api/types'
import { HoverCard, HoverCardContent, HoverCardTrigger } from '@/components/animate-ui/hover-card'
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/animate-ui/dialog'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/tip'
import {
  DIRECTION_LABEL,
  SIDE_LABEL,
  TRIGGER_LABEL,
  fmtMoney,
  fmtQty,
  fmtTime,
  sideLabel,
  signTone,
} from '@/lib/format'
import { TIPS } from '@/lib/tips'
import { cn } from '@/lib/utils'

export function ActivityFeed({
  timeline,
  trades,
  prices,
  preview = 6,
}: {
  timeline: TimelineEntry[]
  trades: Trade[]
  prices: Price[]
  preview?: number
}) {
  const recent = timeline.slice(0, preview)

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-md border border-border/70 bg-card">
      <div className="flex shrink-0 items-center justify-between gap-2 border-b border-border/50 px-2.5 py-1.5">
        <div>
          <h2 className="inline-flex items-center gap-1 text-xs font-semibold uppercase tracking-wide">
            Atividade <Tip text={TIPS.panelActivity} />
          </h2>
          <p className="text-[10px] text-muted-foreground">decisões recentes</p>
        </div>
        <LogModal timeline={timeline} trades={trades} prices={prices} />
      </div>

      <ul className="min-h-0 flex-1 divide-y divide-border/50 overflow-y-auto">
        {recent.length === 0 ? (
          <li className="px-4 py-6 text-center text-sm text-muted-foreground">
            Nenhuma decisão ainda.
          </li>
        ) : (
          recent.map((e, idx) => (
            <li key={`${e.created_at}-${e.symbol}-${idx}`}>
              <HoverCard>
                <HoverCardTrigger asChild>
                  <button
                    type="button"
                    className="flex w-full items-start gap-2 px-3 py-2 text-left transition-colors hover:bg-muted/30"
                  >
                    <Badge
                      tone={e.approved ? 'positive' : 'warning'}
                      className="mt-0.5 shrink-0 px-1.5 py-0 text-[10px]"
                    >
                      {e.approved ? 'ok' : 'block'}
                    </Badge>
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm font-medium">
                        {e.symbol}{' '}
                        <span className="font-normal text-accent">{e.strategy_id}</span>
                      </p>
                      <p className="truncate text-[11px] text-muted-foreground">
                        {e.reason ||
                          (TRIGGER_LABEL[e.trigger] || e.trigger) +
                            (e.signal_direction
                              ? ` · ${DIRECTION_LABEL[e.signal_direction] || e.signal_direction}`
                              : '')}
                      </p>
                    </div>
                    <span className="shrink-0 text-[10px] text-muted-foreground">
                      {fmtTime(e.created_at)}
                    </span>
                  </button>
                </HoverCardTrigger>
                <HoverCardContent side="left" align="start" className="w-80">
                  <TimelineHover entry={e} />
                </HoverCardContent>
              </HoverCard>
            </li>
          ))
        )}
      </ul>
    </div>
  )
}

function LogModal({
  timeline,
  trades,
  prices,
}: {
  timeline: TimelineEntry[]
  trades: Trade[]
  prices: Price[]
}) {
  return (
    <Dialog>
      <DialogTrigger asChild>
        <Button variant="outline" size="icon" className="h-7 w-7" aria-label="Histórico completo">
          <Expand className="h-3.5 w-3.5" />
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Histórico</DialogTitle>
        </DialogHeader>
        <DialogBody className="space-y-6">
          <ModalBlock title={`Timeline (${timeline.length})`}>
            {timeline.length === 0 ? (
              <Empty>Nenhuma decisão.</Empty>
            ) : (
              <ul className="divide-y divide-border/50 rounded-lg border border-border/60">
                {timeline.map((e, idx) => (
                  <li key={`${e.created_at}-${e.symbol}-${idx}`} className="px-3 py-2.5 text-sm">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-semibold">{e.symbol}</span>
                      <span className="text-accent">{e.strategy_id}</span>
                      <Badge tone={e.approved ? 'positive' : 'warning'}>
                        {e.approved ? 'aprovado' : 'rejeitado'}
                      </Badge>
                      <span className="ml-auto text-[11px] text-muted-foreground">
                        {fmtTime(e.created_at)}
                      </span>
                    </div>
                    <p className="mt-1 text-xs text-muted-foreground">
                      {TRIGGER_LABEL[e.trigger] || e.trigger}
                      {e.reason ? ` — ${e.reason}` : ''}
                    </p>
                    {e.approved && e.order_side ? (
                      <p className="mt-1 font-mono text-[11px] text-muted-foreground">
                        {SIDE_LABEL[e.order_side.toLowerCase()] || e.order_side}{' '}
                        {fmtQty(e.order_quantity)}
                        {e.fill_price != null ? ` @ ${fmtMoney(e.fill_price)}` : ''}
                        {e.pnl_net != null ? ` · P&L ${fmtMoney(e.pnl_net)}` : ''}
                      </p>
                    ) : null}
                  </li>
                ))}
              </ul>
            )}
          </ModalBlock>

          <ModalBlock title={`Trades fechados (${trades.length})`}>
            {trades.length === 0 ? (
              <Empty>Nenhum trade fechado.</Empty>
            ) : (
              <ul className="divide-y divide-border/50 rounded-lg border border-border/60">
                {trades.map((t, idx) => {
                  const tone = signTone(t.pnl_net)
                  return (
                    <li
                      key={`${t.symbol}-${t.closed_at}-${idx}`}
                      className="flex flex-wrap items-center gap-2 px-3 py-2.5 text-sm"
                    >
                      <span className="font-semibold">{t.symbol}</span>
                      <span className="text-xs text-muted-foreground">
                        {t.strategy_id} · {sideLabel(t.side)} · {fmtTime(t.closed_at)}
                      </span>
                      <span
                        className={cn(
                          'ml-auto font-mono text-xs font-semibold',
                          tone === 'positive' && 'text-positive',
                          tone === 'negative' && 'text-negative',
                        )}
                      >
                        {fmtMoney(t.pnl_net)}
                      </span>
                    </li>
                  )
                })}
              </ul>
            )}
          </ModalBlock>

          <ModalBlock title={`Preços (${prices.length})`}>
            {prices.length === 0 ? (
              <Empty>Nenhum preço.</Empty>
            ) : (
              <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
                {prices.map((p) => (
                  <div
                    key={p.symbol}
                    className="rounded-lg border border-border/60 bg-surface/50 px-3 py-2"
                  >
                    <p className="text-xs text-muted-foreground">
                      {p.symbol} <Tip text={TIPS.price} />
                    </p>
                    <p className="font-mono text-sm font-semibold">{fmtMoney(p.price)}</p>
                    <p className="text-[10px] text-muted-foreground">{fmtTime(p.updated_at)}</p>
                  </div>
                ))}
              </div>
            )}
          </ModalBlock>
        </DialogBody>
      </DialogContent>
    </Dialog>
  )
}

function TimelineHover({ entry: e }: { entry: TimelineEntry }) {
  return (
    <div className="space-y-2 text-xs">
      <p className="text-sm font-semibold">
        {e.symbol} · {e.strategy_id}
      </p>
      <p className="text-muted-foreground">
        Gatilho <Tip text={TIPS.trigger} />: {TRIGGER_LABEL[e.trigger] || e.trigger}
        {e.signal_confidence != null ? (
          <>
            {' '}
            · {(e.signal_confidence * 100).toFixed(0)}%
            <Tip text={TIPS.confidence} />
          </>
        ) : null}
      </p>
      {e.approved && e.order_side ? (
        <p className="rounded-lg bg-muted/50 px-2.5 py-2 font-mono">
          {SIDE_LABEL[e.order_side.toLowerCase()] || e.order_side} {fmtQty(e.order_quantity)}
          {e.fill_price != null ? ` @ ${fmtMoney(e.fill_price)}` : ''}
          {e.pnl_net != null ? ` · P&L ${fmtMoney(e.pnl_net)}` : ''}
        </p>
      ) : (
        <p className="text-warning">{e.reason ?? 'Bloqueado pelo risco'}</p>
      )}
    </div>
  )
}

function ModalBlock({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="space-y-2">
      <h3 className="text-sm font-semibold">{title}</h3>
      {children}
    </section>
  )
}

function Empty({ children }: { children: ReactNode }) {
  return <p className="text-sm text-muted-foreground">{children}</p>
}
