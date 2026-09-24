import type { StrategyPerformance } from '@/api/types'
import { Stagger, StaggerItem } from '@/components/animate-ui/motion'
import { HoverCard, HoverCardContent, HoverCardTrigger } from '@/components/animate-ui/hover-card'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Tip } from '@/components/tip'
import { fmtMoney, fmtPct, num, signTone } from '@/lib/format'
import { TIPS } from '@/lib/tips'
import { cn } from '@/lib/utils'

export function PerformanceSection({ rows }: { rows: StrategyPerformance[] }) {
  const sorted = [...rows].sort((a, b) => (num(b.net_pnl) ?? 0) - (num(a.net_pnl) ?? 0))
  const peak = Math.max(1, ...sorted.map((s) => Math.abs(num(s.net_pnl) ?? 0)))

  return (
    <section className="space-y-2.5">
      <div>
        <h2 className="text-base font-semibold tracking-tight">Desempenho por estratégia</h2>
        <p className="text-xs text-muted-foreground">Hover no card para métricas</p>
      </div>

      {sorted.length === 0 ? (
        <Card>
          <CardContent className="p-8 text-center text-sm text-muted-foreground">
            Nenhum trade fechado ainda para calcular desempenho.
          </CardContent>
        </Card>
      ) : (
        <Stagger className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {sorted.map((s) => {
            const tone = signTone(s.net_pnl)
            const bar = (Math.abs(num(s.net_pnl) ?? 0) / peak) * 100
            return (
              <StaggerItem key={s.strategy_id}>
                <HoverCard>
                  <HoverCardTrigger asChild>
                    <Card className="cursor-default transition-colors hover:border-accent/45">
                      <CardHeader className="flex-row items-start justify-between gap-2 space-y-0">
                        <div>
                          <CardTitle className="text-accent">{s.strategy_id}</CardTitle>
                          <p className="text-xs text-muted-foreground">
                            {s.total_trades} trades · acerto {fmtPct(s.win_rate)}
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
                          {fmtMoney(s.net_pnl)}
                        </Badge>
                      </CardHeader>
                      <CardContent>
                        <div className="h-2 overflow-hidden rounded-full bg-muted">
                          <div
                            className={cn(
                              'h-full rounded-full transition-all duration-700',
                              tone === 'negative' ? 'bg-negative' : 'bg-positive',
                            )}
                            style={{ width: `${bar}%` }}
                          />
                        </div>
                      </CardContent>
                    </Card>
                  </HoverCardTrigger>
                  <HoverCardContent className="w-96" side="bottom">
                    <p className="mb-3 font-semibold text-accent">{s.strategy_id}</p>
                    <div className="grid grid-cols-2 gap-2 text-xs">
                      <Stat label="Trades" tip={TIPS.trades} value={String(s.total_trades)} />
                      <Stat label="Ganhos" tip={TIPS.winners} value={String(s.winners)} />
                      <Stat label="Perdas" tip={TIPS.losers} value={String(s.losers)} />
                      <Stat label="Acerto" tip={TIPS.winRate} value={fmtPct(s.win_rate)} />
                      <Stat
                        label="Bruto"
                        tip={TIPS.grossPnl}
                        value={fmtMoney(s.gross_pnl)}
                        tone={signTone(s.gross_pnl)}
                      />
                      <Stat
                        label="Líquido"
                        tip={TIPS.netPnl}
                        value={fmtMoney(s.net_pnl)}
                        tone={signTone(s.net_pnl)}
                      />
                      <Stat
                        label="Ganho médio"
                        tip={TIPS.avgWin}
                        value={fmtMoney(s.average_win)}
                        tone="positive"
                      />
                      <Stat
                        label="Perda média"
                        tip={TIPS.avgLoss}
                        value={fmtMoney(s.average_loss)}
                        tone="negative"
                      />
                      <Stat
                        label="Profit factor"
                        tip={TIPS.profitFactor}
                        value={s.profit_factor != null ? fmtMoney(s.profit_factor) : '—'}
                      />
                      <Stat
                        label="Max DD"
                        tip={TIPS.maxDrawdown}
                        value={fmtMoney(s.max_drawdown)}
                        tone="negative"
                      />
                    </div>
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

function Stat({
  label,
  tip,
  value,
  tone,
}: {
  label: string
  tip: string
  value: string
  tone?: 'positive' | 'negative' | 'neutral'
}) {
  return (
    <div className="rounded-lg border border-border/50 bg-surface/80 p-2.5">
      <div className="mb-1 flex items-center gap-1 text-[11px] text-muted-foreground">
        {label}
        <Tip text={tip} />
      </div>
      <p
        className={cn(
          'font-mono text-sm font-semibold tabular-nums',
          tone === 'positive' && 'text-positive',
          tone === 'negative' && 'text-negative',
        )}
      >
        {value}
      </p>
    </div>
  )
}
