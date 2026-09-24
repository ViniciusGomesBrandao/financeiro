import type { ReactNode } from 'react'
import type { DashboardData, OpenPosition, StrategyPerformance } from '@/api/types'
import { FadeIn, Stagger, StaggerItem } from '@/components/animate-ui/motion'
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/animate-ui/dialog'
import { ActivityFeed } from '@/components/dashboard/activity-feed'
import {
  InlineAsset,
  InlineMoney,
  InlineStrategy,
  InlineTime,
  PlainSentence,
} from '@/components/narrative/inline'
import { Badge } from '@/components/ui/badge'
import { Tip } from '@/components/tip'
import {
  fmtDuration,
  fmtMoney,
  fmtPct,
  fmtQty,
  fmtTime,
  num,
  sideLabel,
  signTone,
} from '@/lib/format'
import {
  lastActionNarrative,
  nowSummary,
  openPositionNarrative,
} from '@/lib/narratives'
import { strategyGuide } from '@/lib/strategy-guides'
import { TIPS } from '@/lib/tips'
import { cn } from '@/lib/utils'

export function WorkspaceResumo({ data }: { data: DashboardData }) {
  const { overview, positions, timeline, trades, prices, performance } = data
  const summary = nowSummary(overview, positions)
  const last = lastActionNarrative(timeline[0] ?? null)
  const strategies = sortedStrategies(performance)
  const peak = strategyPeak(strategies)

  return (
    <FadeIn className="flex h-full min-h-0 flex-col gap-2 overflow-hidden">
      <MetricsStrip overview={overview} summary={summary} />

      <div className="grid min-h-0 flex-1 gap-2 lg:grid-cols-12">
        <section className="flex min-h-0 flex-col gap-2 lg:col-span-4">
          <Panel
            title="Posições"
            tip={TIPS.panelPositions}
            subtitle={`${summary.openCount} abertas`}
            className="min-h-0 flex-[1.1]"
          >
            {summary.watchingOnly ? (
              <PlainSentence className="p-3 text-sm text-muted-foreground">
                Só olhando o mercado. Nenhuma compra aberta.
              </PlainSentence>
            ) : (
              <Stagger className="grid gap-1.5 overflow-y-auto p-2">
                {positions.map((p) => (
                  <StaggerItem key={`${p.symbol}-${p.strategy_id}-${p.opened_at}`}>
                    <PositionCard position={p} />
                  </StaggerItem>
                ))}
              </Stagger>
            )}
            <div className="mt-auto border-t border-border/50 px-2.5 py-2">
              <p className="mb-0.5 text-[10px] uppercase tracking-wide text-muted-foreground">
                Última ação
              </p>
              <LastActionLine last={last} />
            </div>
          </Panel>

          <Panel
            title="Trades fechados"
            tip={TIPS.panelClosedTrades}
            subtitle={`${trades.length} no total`}
            className="min-h-0 flex-1"
          >
            <TradesList trades={trades.slice(0, 14)} />
          </Panel>
        </section>

        <Panel
          title="Estratégias"
          tip={TIPS.panelStrategies}
          className="min-h-0 lg:col-span-4"
        >
          <StrategiesList strategies={strategies} peak={peak} />
        </Panel>

        <aside className="min-h-0 lg:col-span-4">
          <ActivityFeed timeline={timeline} trades={trades} prices={prices} preview={40} />
        </aside>
      </div>

      <MarketBar prices={prices} />
    </FadeIn>
  )
}

export function WorkspaceOperacoes({ data }: { data: DashboardData }) {
  const { positions, performance, trades } = data
  const summary = nowSummary(data.overview, positions)
  const strategies = sortedStrategies(performance)
  const peak = strategyPeak(strategies)

  return (
    <FadeIn className="grid h-full min-h-0 gap-2 overflow-hidden lg:grid-cols-12">
      <Panel
        title="Posições"
        tip={TIPS.panelPositions}
        subtitle={`${summary.openCount} abertas`}
        className="min-h-0 lg:col-span-4"
      >
        {summary.watchingOnly ? (
          <p className="p-3 text-sm text-muted-foreground">Nenhuma posição aberta.</p>
        ) : (
          <Stagger className="grid gap-1.5 overflow-y-auto p-2">
            {positions.map((p) => (
              <StaggerItem key={`${p.symbol}-${p.strategy_id}-${p.opened_at}`}>
                <PositionCard position={p} />
              </StaggerItem>
            ))}
          </Stagger>
        )}
      </Panel>

      <Panel
        title="Estratégias"
        tip={TIPS.panelStrategies}
        className="min-h-0 lg:col-span-4"
      >
        <StrategiesList strategies={strategies} peak={peak} />
      </Panel>

      <Panel
        title="Trades fechados"
        tip={TIPS.panelClosedTrades}
        subtitle={`${trades.length} no total`}
        className="min-h-0 lg:col-span-4"
      >
        <TradesList trades={trades} />
      </Panel>
    </FadeIn>
  )
}

export function WorkspaceLog({ data }: { data: DashboardData }) {
  return (
    <FadeIn className="flex h-full min-h-0 flex-col gap-2 overflow-hidden">
      <div className="min-h-0 flex-1">
        <ActivityFeed
          timeline={data.timeline}
          trades={data.trades}
          prices={data.prices}
          preview={80}
        />
      </div>
      <MarketBar prices={data.prices} />
    </FadeIn>
  )
}

function MetricsStrip({
  overview,
  summary,
}: {
  overview: DashboardData['overview']
  summary: ReturnType<typeof nowSummary>
}) {
  return (
    <div className="grid shrink-0 grid-cols-4 gap-1.5 xl:grid-cols-8">
      <Stat
        label="Patrimônio"
        tip={TIPS.equity}
        emphasize
        value={<InlineMoney value={summary.equity} animate />}
      />
      <Stat label="Caixa" tip={TIPS.cash} value={<InlineMoney value={overview.cash} animate />} />
      <Stat
        label="Hoje"
        tip={TIPS.pnlToday}
        value={<InlineMoney value={summary.pnlToday} signed animate />}
        tone={signTone(summary.pnlToday)}
      />
      <Stat
        label="Não realizado"
        tip={TIPS.unrealized}
        value={<InlineMoney value={overview.unrealized_pnl} signed animate />}
        tone={signTone(overview.unrealized_pnl)}
      />
      <Stat
        label="P&L total"
        tip={TIPS.pnlTotal}
        value={<InlineMoney value={overview.realized_pnl_total} signed animate />}
        tone={signTone(overview.realized_pnl_total)}
      />
      <Stat
        label="Retorno"
        tip={TIPS.returnPct}
        value={fmtPct(overview.return_pct)}
        tone={signTone(overview.return_pct)}
      />
      <Stat
        label="Em risco"
        tip={TIPS.exposedCapital}
        value={
          <>
            <InlineMoney value={summary.exposed} animate />
            {summary.exposedPct != null ? (
              <span className="ml-1 text-[10px] font-sans text-muted-foreground">
                {summary.exposedPct.toLocaleString('pt-BR', { maximumFractionDigits: 1 })}%
              </span>
            ) : null}
          </>
        }
      />
      <Stat
        label="Max DD"
        tip={TIPS.maxDrawdown}
        value={fmtMoney(overview.max_drawdown)}
        tone="negative"
      />
    </div>
  )
}

function PositionCard({ position: p }: { position: OpenPosition }) {
  const n = openPositionNarrative(p)
  const verb =
    n.unrealized !== null && n.unrealized < 0
      ? 'perdendo'
      : n.unrealized !== null && n.unrealized > 0
        ? 'rendendo'
        : 'empatado'
  const tone = signTone(n.unrealized)

  return (
    <Dialog>
      <DialogTrigger asChild>
        <button
          type="button"
          className="w-full rounded-md border border-border/50 bg-surface/40 px-2.5 py-2 text-left hover:border-accent/40"
        >
          <div className="flex items-center justify-between gap-2">
            <span className="text-sm font-semibold">
              <InlineAsset symbol={n.symbol} />
            </span>
            <Badge
              tone={tone === 'positive' ? 'positive' : tone === 'negative' ? 'negative' : 'neutral'}
              className="px-1.5 py-0 text-[10px]"
            >
              <InlineMoney value={n.unrealized} signed animate />
            </Badge>
          </div>
          <p className="mt-0.5 text-[11px] text-muted-foreground">
            <InlineStrategy id={n.strategyId} /> · {fmtDuration(p.opened_at)} · {verb}
          </p>
          <p className="text-[11px] text-muted-foreground">
            <InlineTime iso={n.openedAt} /> · <InlineMoney value={n.entryNotional} />
          </p>
        </button>
      </DialogTrigger>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>
            {p.symbol} · {p.strategy_id}
          </DialogTitle>
          <DialogDescription>
            Posição ainda aberta. Valores vêm da API — esta tela só apresenta.
          </DialogDescription>
        </DialogHeader>
        <DialogBody className="space-y-3">
          <p className="text-sm text-muted-foreground">
            {sideLabel(p.side)} · aberta {fmtTime(p.opened_at)} · {fmtDuration(p.opened_at)}
          </p>
          <div className="grid grid-cols-2 gap-2 text-sm">
            <Fact label="Quantidade" value={fmtQty(p.quantity)} />
            <Fact label="Entrada" value={fmtMoney(p.entry_price)} />
            <Fact
              label="Atual"
              value={p.current_price != null ? fmtMoney(p.current_price) : '—'}
            />
            <Fact
              label="P&L não realizado"
              value={fmtMoney(p.unrealized_pnl)}
              tone={signTone(p.unrealized_pnl)}
            />
          </div>
        </DialogBody>
      </DialogContent>
    </Dialog>
  )
}

function StrategiesList({
  strategies,
  peak,
}: {
  strategies: StrategyPerformance[]
  peak: number
}) {
  if (strategies.length === 0) {
    return <p className="p-3 text-sm text-muted-foreground">Sem trades fechados ainda.</p>
  }

  return (
    <Stagger className="min-h-0 flex-1 space-y-1.5 overflow-y-auto p-2">
      {strategies.map((s) => {
        const tone = signTone(s.net_pnl)
        const bar = (Math.abs(num(s.net_pnl) ?? 0) / peak) * 100
        const guide = strategyGuide(s.strategy_id)
        return (
          <StaggerItem key={s.strategy_id}>
            <Dialog>
              <DialogTrigger asChild>
                <button
                  type="button"
                  className="w-full rounded-md border border-border/50 bg-surface/40 px-2.5 py-2 text-left hover:border-accent/40"
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-sm font-semibold text-accent">{s.strategy_id}</span>
                    <span
                      className={cn(
                        'font-mono text-xs font-semibold',
                        tone === 'positive' && 'text-positive',
                        tone === 'negative' && 'text-negative',
                      )}
                    >
                      {fmtMoney(s.net_pnl)}
                    </span>
                  </div>
                  <p className="mt-0.5 text-[11px] text-muted-foreground">
                    {s.total_trades} trades · acerto {fmtPct(s.win_rate)} · {s.winners}W / {s.losers}
                    L
                  </p>
                  <div className="mt-1.5 h-1 overflow-hidden rounded-full bg-muted">
                    <div
                      className={cn(
                        'h-full rounded-full',
                        tone === 'negative' ? 'bg-negative' : 'bg-positive',
                      )}
                      style={{ width: `${bar}%` }}
                    />
                  </div>
                </button>
              </DialogTrigger>
              <DialogContent>
                <DialogHeader>
                  <DialogTitle className="text-accent">
                    {guide.title}{' '}
                    <span className="font-mono text-sm text-muted-foreground">({s.strategy_id})</span>
                  </DialogTitle>
                  <DialogDescription>Como a regra funciona e o desempenho atual.</DialogDescription>
                </DialogHeader>
                <DialogBody className="space-y-4 text-sm">
                  <Block title="Ideia">{guide.idea}</Block>
                  <Block title="Como opera">{guide.how}</Block>
                  <Block title="Quando costuma falhar">{guide.failsWhen}</Block>
                  <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
                    <Fact label="Trades" value={String(s.total_trades)} />
                    <Fact label="Acerto" value={fmtPct(s.win_rate)} />
                    <Fact label="Bruto" value={fmtMoney(s.gross_pnl)} tone={signTone(s.gross_pnl)} />
                    <Fact label="Líquido" value={fmtMoney(s.net_pnl)} tone={signTone(s.net_pnl)} />
                    <Fact label="Ganho méd." value={fmtMoney(s.average_win)} tone="positive" />
                    <Fact label="Perda méd." value={fmtMoney(s.average_loss)} tone="negative" />
                    <Fact
                      label="Profit factor"
                      value={s.profit_factor != null ? fmtMoney(s.profit_factor) : '—'}
                    />
                    <Fact label="Max DD" value={fmtMoney(s.max_drawdown)} tone="negative" />
                  </div>
                </DialogBody>
              </DialogContent>
            </Dialog>
          </StaggerItem>
        )
      })}
    </Stagger>
  )
}

function TradesList({ trades }: { trades: DashboardData['trades'] }) {
  if (trades.length === 0) {
    return <p className="p-3 text-sm text-muted-foreground">Nenhum trade fechado.</p>
  }

  return (
    <ul className="min-h-0 flex-1 divide-y divide-border/40 overflow-y-auto">
      {trades.map((t, i) => {
        const tone = signTone(t.pnl_net)
        return (
          <li
            key={`${t.symbol}-${t.closed_at}-${i}`}
            className="flex items-center gap-2 px-2.5 py-1.5 text-xs"
          >
            <span className="font-semibold">{t.symbol}</span>
            <span className="truncate text-muted-foreground">{t.strategy_id}</span>
            <span className="ml-auto shrink-0 text-[10px] text-muted-foreground">
              {fmtTime(t.closed_at)}
            </span>
            <span
              className={cn(
                'shrink-0 font-mono font-semibold',
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
  )
}

function MarketBar({ prices }: { prices: DashboardData['prices'] }) {
  if (prices.length === 0) return null
  return (
    <div className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-1 rounded-md border border-border/60 bg-card px-3 py-1.5">
      <span className="inline-flex items-center gap-1 text-[10px] font-medium uppercase tracking-wide text-muted-foreground">
        Mercado <Tip text={TIPS.panelMarket} />
      </span>
      {prices.map((p) => (
        <span key={p.symbol} className="inline-flex items-baseline gap-1.5 text-xs">
          <span className="text-muted-foreground">{p.symbol}</span>
          <span className="font-mono font-semibold tabular-nums">{fmtMoney(p.price)}</span>
          <span className="text-[10px] text-muted-foreground">{fmtTime(p.updated_at)}</span>
        </span>
      ))}
    </div>
  )
}

function Panel({
  title,
  tip,
  subtitle,
  children,
  className,
}: {
  title: string
  tip?: string
  subtitle?: string
  children: ReactNode
  className?: string
}) {
  return (
    <div
      className={cn(
        'flex min-h-0 flex-col overflow-hidden rounded-md border border-border/70 bg-card',
        className,
      )}
    >
      <div className="flex shrink-0 items-baseline justify-between gap-2 border-b border-border/50 px-2.5 py-1.5">
        <h2 className="inline-flex items-center gap-1 text-xs font-semibold uppercase tracking-wide">
          {title}
          {tip ? <Tip text={tip} /> : null}
        </h2>
        {subtitle ? <span className="text-[10px] text-muted-foreground">{subtitle}</span> : null}
      </div>
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">{children}</div>
    </div>
  )
}

function Stat({
  label,
  tip,
  value,
  tone,
  emphasize,
}: {
  label: string
  tip: string
  value: ReactNode
  tone?: 'positive' | 'negative' | 'neutral'
  emphasize?: boolean
}) {
  return (
    <div
      className={cn(
        'rounded-md border border-border/60 bg-card px-2.5 py-1.5',
        emphasize && 'border-accent/35 bg-gradient-to-br from-accent/10 to-card',
      )}
    >
      <div className="flex items-center gap-1 text-[10px] uppercase tracking-wide text-muted-foreground">
        {label}
        <Tip text={tip} />
      </div>
      <div
        className={cn(
          'font-mono text-sm font-semibold tabular-nums leading-tight sm:text-base',
          tone === 'positive' && 'text-positive',
          tone === 'negative' && 'text-negative',
        )}
      >
        {value}
      </div>
    </div>
  )
}

function Fact({
  label,
  value,
  tone,
}: {
  label: string
  value: string
  tone?: 'positive' | 'negative' | 'neutral'
}) {
  return (
    <div className="rounded-md bg-muted/35 px-2.5 py-2">
      <p className="text-[10px] text-muted-foreground">{label}</p>
      <p
        className={cn(
          'font-mono text-sm font-semibold',
          tone === 'positive' && 'text-positive',
          tone === 'negative' && 'text-negative',
        )}
      >
        {value}
      </p>
    </div>
  )
}

function Block({ title, children }: { title: string; children: string }) {
  return (
    <div>
      <p className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
        {title}
      </p>
      <p className="leading-relaxed text-foreground/90">{children}</p>
    </div>
  )
}

function LastActionLine({ last }: { last: ReturnType<typeof lastActionNarrative> }) {
  if (last.kind === 'none') {
    return <p className="text-xs text-muted-foreground">Nenhuma decisão ainda.</p>
  }
  if (last.kind === 'rejected') {
    return (
      <PlainSentence className="text-xs">
        Tentou {last.directionWord} <InlineAsset symbol={last.symbol} /> /{' '}
        <InlineStrategy id={last.strategyId} />
        {last.reason ? <> — {last.reason}</> : null}
      </PlainSentence>
    )
  }
  return (
    <PlainSentence className="text-xs">
      {last.directionWord} <InlineAsset symbol={last.symbol} /> /{' '}
      <InlineStrategy id={last.strategyId} /> ok
      {last.fillPrice ? (
        <>
          {' '}
          @ <InlineMoney value={last.fillPrice} />
        </>
      ) : null}
      {last.pnlNet !== null ? (
        <>
          {' '}
          · <InlineMoney value={last.pnlNet} signed />
        </>
      ) : null}
    </PlainSentence>
  )
}

function sortedStrategies(rows: StrategyPerformance[]) {
  return [...rows].sort((a, b) => (num(b.net_pnl) ?? 0) - (num(a.net_pnl) ?? 0))
}

function strategyPeak(strategies: StrategyPerformance[]) {
  return Math.max(1, ...strategies.map((s) => Math.abs(num(s.net_pnl) ?? 0)))
}
