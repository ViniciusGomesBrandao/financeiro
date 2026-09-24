import type { TimelineEntry } from '@/api/types'
import { Stagger, StaggerItem } from '@/components/animate-ui/motion'
import { HoverCard, HoverCardContent, HoverCardTrigger } from '@/components/animate-ui/hover-card'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent } from '@/components/ui/card'
import { Tip } from '@/components/tip'
import {
  DIRECTION_LABEL,
  SIDE_LABEL,
  TRIGGER_LABEL,
  fmtMoney,
  fmtQty,
  fmtTime,
} from '@/lib/format'
import { TIPS } from '@/lib/tips'

export function TimelineSection({ entries }: { entries: TimelineEntry[] }) {
  return (
    <section className="space-y-3">
      <div>
        <h2 className="text-lg font-semibold tracking-tight">Timeline</h2>
        <p className="text-sm text-muted-foreground">Hover para ver a execução completa</p>
      </div>

      {entries.length === 0 ? (
        <Card>
          <CardContent className="p-8 text-center text-sm text-muted-foreground">
            Nenhuma decisão registrada.
          </CardContent>
        </Card>
      ) : (
        <Stagger className="grid gap-3 md:grid-cols-2">
          {entries.slice(0, 24).map((e, idx) => (
            <StaggerItem key={`${e.created_at}-${e.symbol}-${idx}`}>
              <HoverCard>
                <HoverCardTrigger asChild>
                  <Card className="cursor-default transition-colors hover:border-accent/40">
                    <CardContent className="space-y-2 p-4">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="font-semibold">{e.symbol}</span>
                        <span className="text-sm text-accent">{e.strategy_id}</span>
                        <Badge tone={e.approved ? 'positive' : 'warning'} className="gap-1">
                          {e.approved ? 'aprovado' : 'rejeitado'}
                          <Tip text={e.approved ? TIPS.approved : TIPS.rejected} />
                        </Badge>
                        <span className="ml-auto text-[11px] text-muted-foreground">
                          {fmtTime(e.created_at)}
                        </span>
                      </div>
                      <p className="text-sm text-muted-foreground">
                        {TRIGGER_LABEL[e.trigger] || e.trigger}
                        {e.signal_direction
                          ? ` · ${DIRECTION_LABEL[e.signal_direction] || e.signal_direction}`
                          : ''}
                        {e.reason ? ` — ${e.reason}` : ''}
                      </p>
                    </CardContent>
                  </Card>
                </HoverCardTrigger>
                <HoverCardContent side="bottom" align="start" className="w-96">
                  <p className="mb-2 font-semibold">
                    {e.symbol} · {e.strategy_id}
                  </p>
                  <p className="mb-2 text-xs text-muted-foreground">
                    Gatilho <Tip text={TIPS.trigger} />: {TRIGGER_LABEL[e.trigger] || e.trigger}
                    {e.signal_confidence != null ? (
                      <>
                        {' '}
                        · confiança {(e.signal_confidence * 100).toFixed(0)}%
                        <Tip text={TIPS.confidence} />
                      </>
                    ) : null}
                  </p>
                  {e.approved && e.order_side ? (
                    <p className="rounded-lg bg-muted/50 px-3 py-2 font-mono text-xs">
                      {SIDE_LABEL[e.order_side.toLowerCase()] || e.order_side} {fmtQty(e.order_quantity)}
                      {e.fill_price != null ? ` @ ${fmtMoney(e.fill_price)}` : ''}
                      {e.fee != null ? ` · fee ${fmtMoney(e.fee)}` : ''}
                      {e.spread_cost != null ? ` · spread ${fmtMoney(e.spread_cost)}` : ''}
                      {e.slippage_cost != null ? ` · slippage ${fmtMoney(e.slippage_cost)}` : ''}
                      {e.pnl_net != null ? ` · P&L ${fmtMoney(e.pnl_net)}` : ''}
                    </p>
                  ) : (
                    <p className="text-xs text-warning">{e.reason ?? 'Bloqueado pelo risco'}</p>
                  )}
                </HoverCardContent>
              </HoverCard>
            </StaggerItem>
          ))}
        </Stagger>
      )}
    </section>
  )
}
