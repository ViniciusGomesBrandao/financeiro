import type { Trade } from '@/api/types'
import { Stagger, StaggerItem } from '@/components/animate-ui/motion'
import { HoverCard, HoverCardContent, HoverCardTrigger } from '@/components/animate-ui/hover-card'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Tip } from '@/components/tip'
import { fmtMoney, fmtQty, fmtTime, sideLabel, signTone } from '@/lib/format'
import { TIPS } from '@/lib/tips'
import { cn } from '@/lib/utils'

export function TradesSection({ trades }: { trades: Trade[] }) {
  return (
    <section className="space-y-3">
      <div>
        <h2 className="text-lg font-semibold tracking-tight">Trades fechados</h2>
        <p className="text-sm text-muted-foreground">Hover para custos e resultado</p>
      </div>

      {trades.length === 0 ? (
        <Card>
          <CardContent className="p-8 text-center text-sm text-muted-foreground">
            Nenhum trade fechado.
          </CardContent>
        </Card>
      ) : (
        <Stagger className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {trades.slice(0, 30).map((t, idx) => {
            const tone = signTone(t.pnl_net)
            return (
              <StaggerItem key={`${t.symbol}-${t.closed_at}-${idx}`}>
                <HoverCard>
                  <HoverCardTrigger asChild>
                    <Card className="cursor-default transition-colors hover:border-accent/45">
                      <CardHeader className="flex-row items-start justify-between gap-2 space-y-0">
                        <div>
                          <CardTitle>{t.symbol}</CardTitle>
                          <p className="text-xs text-muted-foreground">
                            {t.strategy_id} · {fmtTime(t.closed_at)}
                          </p>
                        </div>
                        <Badge
                          tone={
                            tone === 'positive'
                              ? 'positive'
                              : tone === 'negative'
                                ? 'negative'
                                : 'neutral'
                          }
                        >
                          {fmtMoney(t.pnl_net)}
                        </Badge>
                      </CardHeader>
                      <CardContent className="text-xs text-muted-foreground">
                        {sideLabel(t.side)} {fmtQty(t.quantity)} · {fmtMoney(t.entry_price)} →{' '}
                        {fmtMoney(t.exit_price)}
                      </CardContent>
                    </Card>
                  </HoverCardTrigger>
                  <HoverCardContent className="w-80">
                    <p className="mb-2 font-semibold">
                      {t.symbol} · {t.strategy_id}
                    </p>
                    <p className="mb-3 text-xs text-muted-foreground">
                      Entrada {fmtTime(t.opened_at)} → saída {fmtTime(t.closed_at)}
                    </p>
                    <div className="grid grid-cols-2 gap-2 text-xs">
                      <Cost label="Bruto" tip={TIPS.tradeGross} value={fmtMoney(t.pnl_gross)} />
                      <Cost label="Fees" tip={TIPS.fees} value={fmtMoney(t.fees_paid)} />
                      <Cost label="Spread" tip={TIPS.spread} value={fmtMoney(t.spread_paid)} />
                      <Cost label="Slippage" tip={TIPS.slippage} value={fmtMoney(t.slippage_paid)} />
                    </div>
                    <p
                      className={cn(
                        'mt-3 font-mono text-sm font-semibold',
                        tone === 'positive' && 'text-positive',
                        tone === 'negative' && 'text-negative',
                      )}
                    >
                      Líquido {fmtMoney(t.pnl_net)} <Tip text={TIPS.tradeNet} />
                    </p>
                  </HoverCardContent>
                </HoverCard>
              </StaggerItem>
            )
          })}
        </Stagger>
      )}
    </section>
  )
}

function Cost({ label, tip, value }: { label: string; tip: string; value: string }) {
  return (
    <div className="rounded-lg bg-muted/40 px-2.5 py-2">
      <div className="mb-0.5 inline-flex items-center gap-1 text-[11px] text-muted-foreground">
        {label} <Tip text={tip} />
      </div>
      <p className="font-mono font-semibold tabular-nums">{value}</p>
    </div>
  )
}
