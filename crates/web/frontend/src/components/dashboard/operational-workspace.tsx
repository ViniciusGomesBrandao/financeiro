import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { AnimatePresence, motion } from 'motion/react'
import { Bot, Download, Play, Square } from 'lucide-react'
import { downloadTradesCsv } from '@/lib/export-trades-csv'
import { fetchRobotDetail } from '@/api/client'
import type {
  EquityCurvePoint,
  JudgeDecisionJson,
  JudgeEvaluation,
  OperationalData,
  OperationalRobot,
  PnlCurvePoint,
  RobotDetail,
  StrategyCatalogEntry,
  StrategyPerformance,
  StrategySwitch,
  Trade,
} from '@/api/types'
import { FadeIn } from '@/components/animate-ui/motion'
import { SlidingNumber } from '@/components/animate-ui/sliding-number'
import { MiniSeries, toSeries } from '@/components/dashboard/operational/mini-series'
import {
  MetricPip,
  OpsPanel,
  OpsShell,
  StatusPip,
} from '@/components/dashboard/operational/shell'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet'
import {
  DIRECTION_LABEL,
  TRIGGER_LABEL,
  fmtClock,
  fmtHold,
  fmtMoney,
  fmtPct,
  fmtTime,
  num,
  signTone,
} from '@/lib/format'
import {
  judgeReasonSentence,
  judgeStateLabel,
  regimeLabel,
  strategyKindFromInstance,
  switchNarrative,
} from '@/lib/judge-narratives'
import { strategyGuide } from '@/lib/strategy-guides'
import { cn } from '@/lib/utils'

const SYMBOLS = ['BTC/USDT', 'ETH/USDT', 'SOL/USDT']
const TIMEFRAMES = ['1m', '5m', '15m', '30m', '1h', '4h', '1d']

type Props = {
  data: OperationalData
  busy: boolean
  onCreate: (input: {
    id: string
    name: string
    symbol: string
    timeframe: string
    candidate_kinds: string[]
    paper_capital: string
  }) => Promise<void>
  onToggle: (id: string, status: 'running' | 'stopped') => Promise<void>
}

export function WorkspaceOperacional({ data, busy, onCreate, onToggle }: Props) {
  const [selectedId, setSelectedId] = useState<string | null>(data.robots[0]?.id ?? null)
  const selected = data.robots.find((r) => r.id === selectedId) ?? data.robots[0] ?? null
  const [detail, setDetail] = useState<RobotDetail | null>(null)
  const [tradeOpen, setTradeOpen] = useState<Trade | null>(null)

  useEffect(() => {
    if (!selected) {
      setDetail(null)
      return
    }
    let cancelled = false
    const load = async () => {
      try {
        const next = await fetchRobotDetail(selected.id)
        if (!cancelled) setDetail(next)
      } catch {
        if (!cancelled) setDetail(null)
      }
    }
    void load()
    const id = window.setInterval(() => void load(), 4000)
    return () => {
      cancelled = true
      window.clearInterval(id)
    }
  }, [selected?.id, selected?.status, selected?.updated_at, selected?.judge_evaluated_at])

  useEffect(() => {
    if (selected && !data.robots.some((r) => r.id === selected.id)) {
      setSelectedId(data.robots[0]?.id ?? null)
    }
  }, [data.robots, selected])

  const robot = detail?.robot ?? selected

  return (
    <FadeIn className="h-full min-h-0">
      <OpsShell>
        <aside className="flex w-[220px] shrink-0 flex-col border-r border-border/60 bg-card/40">
          <OpsPanel
            title="Robots"
            right={
              <span className="font-mono text-[10px] text-muted-foreground">{data.robots.length}</span>
            }
            className="min-h-0 flex-1"
            bodyClassName="flex min-h-0 flex-col"
          >
            <div className="border-b border-border/40 p-2">
              <CreateRobotDialog catalog={data.catalog} busy={busy} onCreate={onCreate} />
            </div>
            <ul className="min-h-0 flex-1 overflow-y-auto p-1">
              {data.robots.length === 0 ? (
                <li className="p-3 font-mono text-[11px] text-muted-foreground">
                  Nenhum robô. Crie um para operar.
                </li>
              ) : (
                data.robots.map((r) => (
                  <RobotRailItem
                    key={r.id}
                    robot={r}
                    active={r.id === robot?.id}
                    onSelect={() => setSelectedId(r.id)}
                  />
                ))
              )}
            </ul>
          </OpsPanel>
        </aside>

        <main className="flex min-h-0 min-w-0 flex-1 flex-col">
          <AnimatePresence mode="wait">
            {robot ? (
              <motion.div
                key={robot.id}
                initial={{ opacity: 0, x: 8 }}
                animate={{ opacity: 1, x: 0 }}
                exit={{ opacity: 0, x: -6 }}
                transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
                className="flex min-h-0 flex-1 flex-col"
              >
                <RobotHeader robot={robot} busy={busy} onToggle={onToggle} />
                <div className="grid min-h-0 flex-1 grid-cols-12 overflow-hidden">
                  <div className="col-span-8 flex min-h-0 flex-col border-r border-border/50">
                    <OpsPanel title="Equity / P&L" className="h-[140px] shrink-0 border-b border-border/50">
                      <EquityBlock
                        equity={detail?.equity_curve ?? []}
                        pnl={detail?.realized_pnl_curve ?? []}
                      />
                    </OpsPanel>
                    <OpsPanel
                      title="Estado"
                      className="h-[88px] shrink-0 border-b border-border/50"
                      bodyClassName="px-2.5 py-2"
                    >
                      <StateStrip robot={robot} />
                    </OpsPanel>
                    <OpsPanel
                      title="Trades"
                      right={
                        <div className="flex items-center gap-2">
                          <span className="font-mono text-[10px] text-muted-foreground">
                            {detail?.trades.length ?? 0}
                          </span>
                          <Button
                            size="sm"
                            variant="outline"
                            className="h-6 gap-1 px-1.5 text-[10px]"
                            disabled={!detail || detail.trades.length === 0}
                            title="Exportar últimas transações + histórico do Judge (CSV)"
                            onClick={() => {
                              if (!detail || !robot) return
                              downloadTradesCsv(robot, detail)
                            }}
                          >
                            <Download className="h-3 w-3" />
                            CSV
                          </Button>
                        </div>
                      }
                      className="min-h-0 flex-1"
                      bodyClassName="min-h-0 overflow-hidden"
                    >
                      <TradesTable trades={detail?.trades ?? []} onOpen={setTradeOpen} />
                    </OpsPanel>
                  </div>
                  <div className="col-span-4 flex min-h-0 flex-col">
                    <OpsPanel
                      title="Judge"
                      className="min-h-0 flex-[1.1] border-b border-border/50"
                      bodyClassName="min-h-0 overflow-y-auto px-2 py-1.5"
                    >
                      <JudgeTimeline
                        evaluations={detail?.evaluations ?? []}
                        switches={detail?.switches ?? []}
                      />
                    </OpsPanel>
                    <OpsPanel
                      title="Candidatas"
                      className="min-h-0 flex-1"
                      bodyClassName="min-h-0 overflow-y-auto p-1.5"
                    >
                      <CandidateGrid
                        robot={robot}
                        performance={detail?.candidate_performance ?? []}
                        catalog={data.catalog}
                      />
                    </OpsPanel>
                  </div>
                </div>
              </motion.div>
            ) : (
              <motion.div
                key="empty"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                className="flex flex-1 items-center justify-center p-6"
              >
                <p className="font-mono text-xs text-muted-foreground">
                  Selecione ou crie um robô para monitorar.
                </p>
              </motion.div>
            )}
          </AnimatePresence>
        </main>
      </OpsShell>

      <TradeDrawer
        trade={tradeOpen}
        robot={robot}
        detail={detail}
        onClose={() => setTradeOpen(null)}
      />
    </FadeIn>
  )
}

function RobotRailItem({
  robot,
  active,
  onSelect,
}: {
  robot: OperationalRobot
  active: boolean
  onSelect: () => void
}) {
  const pnlTone = signTone(robot.net_pnl)
  const kind = robot.active_strategy_id
    ? strategyGuide(strategyKindFromInstance(robot.active_strategy_id)).title
    : '—'
  return (
    <button
      type="button"
      onClick={onSelect}
      className={cn(
        'mb-0.5 flex w-full flex-col gap-0.5 rounded-sm border px-2 py-1.5 text-left transition-colors',
        active
          ? 'border-accent/40 bg-accent/10'
          : 'border-transparent hover:border-border/60 hover:bg-muted/30',
      )}
    >
      <div className="flex items-center gap-1.5">
        <StatusPip active={robot.status === 'running'} warning={robot.engine_restart_required} />
        <span className="truncate text-xs font-medium">{robot.name}</span>
        <span
          className={cn(
            'ml-auto font-mono text-[10px] tabular-nums',
            pnlTone === 'positive' && 'text-positive',
            pnlTone === 'negative' && 'text-negative',
            pnlTone === 'neutral' && 'text-muted-foreground',
          )}
        >
          {fmtMoney(robot.net_pnl)}
        </span>
      </div>
      <div className="flex items-center gap-1 font-mono text-[9px] text-muted-foreground">
        <span>{robot.symbol}</span>
        <span>·</span>
        <span>{robot.timeframe}</span>
        <span className="ml-auto truncate text-foreground/70">{kind}</span>
      </div>
    </button>
  )
}

function RobotHeader({
  robot,
  busy,
  onToggle,
}: {
  robot: OperationalRobot
  busy: boolean
  onToggle: Props['onToggle']
}) {
  const running = robot.status === 'running'
  const pnl = num(robot.net_pnl) ?? 0
  const equity = num(robot.equity) ?? num(robot.paper_capital) ?? 0
  return (
    <header className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-2 border-b border-border/60 bg-card/30 px-3 py-2">
      <div className="flex min-w-0 items-center gap-2">
        <Bot className="h-3.5 w-3.5 text-accent" />
        <h2 className="truncate text-sm font-semibold tracking-tight">{robot.name}</h2>
        <Badge tone={running ? 'accent' : 'neutral'}>{running ? 'running' : 'stopped'}</Badge>
        <span className="font-mono text-[10px] text-muted-foreground">
          {robot.symbol} · {robot.timeframe}
        </span>
      </div>
      <div className="flex flex-1 flex-wrap items-end gap-4">
        <MetricPip label="Equity" value={<SlidingNumber number={equity} decimalPlaces={2} />} />
        <MetricPip
          label="P&L"
          tone={signTone(robot.net_pnl)}
          value={<SlidingNumber number={pnl} decimalPlaces={2} />}
        />
        <MetricPip label="Retorno" tone={signTone(robot.return_pct)} value={fmtPct(robot.return_pct)} />
        <MetricPip
          label="Estratégia"
          mono={false}
          value={
            robot.active_strategy_id
              ? strategyGuide(strategyKindFromInstance(robot.active_strategy_id)).title
              : '—'
          }
        />
        <MetricPip
          label="Regime"
          mono={false}
          value={
            robot.market_regime
              ? `${regimeLabel(robot.market_regime)}${
                  robot.regime_strength != null
                    ? ` ${Math.round(robot.regime_strength * 100)}%`
                    : ''
                }`
              : '—'
          }
        />
      </div>
      <Button
        size="sm"
        variant={running ? 'outline' : 'default'}
        disabled={busy}
        onClick={() => void onToggle(robot.id, running ? 'stopped' : 'running')}
      >
        {running ? (
          <>
            <Square className="h-3 w-3" /> Parar
          </>
        ) : (
          <>
            <Play className="h-3 w-3" /> Iniciar
          </>
        )}
      </Button>
    </header>
  )
}

function StateStrip({ robot }: { robot: OperationalRobot }) {
  return (
    <div className="grid grid-cols-3 gap-3">
      <div>
        <div className="font-mono text-[9px] uppercase tracking-[0.12em] text-muted-foreground">
          Estratégia ativa
        </div>
        <div className="text-sm font-medium">
          {robot.active_strategy_id
            ? strategyGuide(strategyKindFromInstance(robot.active_strategy_id)).title
            : 'Nenhuma'}
        </div>
      </div>
      <div>
        <div className="font-mono text-[9px] uppercase tracking-[0.12em] text-muted-foreground">
          Regime
        </div>
        <div className="text-sm font-medium">{regimeLabel(robot.market_regime)}</div>
        {robot.regime_summary ? (
          <p className="mt-0.5 line-clamp-2 text-[10px] text-muted-foreground">{robot.regime_summary}</p>
        ) : null}
      </div>
      <div>
        <div className="font-mono text-[9px] uppercase tracking-[0.12em] text-muted-foreground">
          Por quê
        </div>
        <p className="text-[11px] leading-snug text-foreground/90">{robot.active_why}</p>
      </div>
    </div>
  )
}

function EquityBlock({
  equity,
  pnl,
}: {
  equity: EquityCurvePoint[]
  pnl: PnlCurvePoint[]
}) {
  const equitySeries = useMemo(() => toSeries(equity, 'equity'), [equity])
  const pnlSeries = useMemo(() => toSeries(pnl, 'cumulative_pnl'), [pnl])
  const lastEq = equitySeries.at(-1)?.value
  const lastPnl = pnlSeries.at(-1)?.value
  return (
    <div className="grid h-full grid-cols-2">
      <div className="flex flex-col border-r border-border/40 px-2 py-1.5">
        <div className="flex items-baseline justify-between">
          <span className="font-mono text-[9px] uppercase tracking-[0.12em] text-muted-foreground">
            Equity
          </span>
          <span className="font-mono text-[11px] tabular-nums">
            {lastEq != null ? fmtMoney(lastEq) : '—'}
          </span>
        </div>
        <div className="min-h-0 flex-1">
          <MiniSeries points={equitySeries} />
        </div>
      </div>
      <div className="flex flex-col px-2 py-1.5">
        <div className="flex items-baseline justify-between">
          <span className="font-mono text-[9px] uppercase tracking-[0.12em] text-muted-foreground">
            P&L acum.
          </span>
          <span
            className={cn(
              'font-mono text-[11px] tabular-nums',
              signTone(lastPnl) === 'positive' && 'text-positive',
              signTone(lastPnl) === 'negative' && 'text-negative',
            )}
          >
            {lastPnl != null ? fmtMoney(lastPnl) : '—'}
          </span>
        </div>
        <div className="min-h-0 flex-1">
          <MiniSeries
            points={pnlSeries}
            strokeClass={(lastPnl ?? 0) >= 0 ? 'stroke-positive' : 'stroke-negative'}
            fillClass={(lastPnl ?? 0) >= 0 ? 'fill-positive/10' : 'fill-negative/10'}
          />
        </div>
      </div>
    </div>
  )
}

function TradesTable({ trades, onOpen }: { trades: Trade[]; onOpen: (t: Trade) => void }) {
  if (trades.length === 0) {
    return <p className="p-3 font-mono text-[11px] text-muted-foreground">Nenhum trade fechado.</p>
  }
  return (
    <div className="h-full overflow-auto">
      <table className="w-full border-collapse text-left font-mono text-[10px]">
        <thead className="sticky top-0 bg-card/95 text-[9px] uppercase tracking-[0.1em] text-muted-foreground">
          <tr className="border-b border-border/50">
            <th className="px-2 py-1.5 font-normal">Horário</th>
            <th className="px-2 py-1.5 font-normal">Estratégia</th>
            <th className="px-2 py-1.5 font-normal">Dir</th>
            <th className="px-2 py-1.5 font-normal">Entrada</th>
            <th className="px-2 py-1.5 font-normal">Saída</th>
            <th className="px-2 py-1.5 font-normal">Dur</th>
            <th className="px-2 py-1.5 font-normal">Resultado</th>
            <th className="px-2 py-1.5 font-normal">Exit</th>
          </tr>
        </thead>
        <tbody>
          {trades.map((t, i) => {
            const tone = signTone(t.pnl_net)
            const kind = strategyGuide(strategyKindFromInstance(t.strategy_id)).title
            return (
              <tr
                key={t.id ?? `${t.opened_at}-${t.strategy_id}-${i}`}
                onClick={() => onOpen(t)}
                className="cursor-pointer border-b border-border/30 hover:bg-accent/5"
              >
                <td className="px-2 py-1 tabular-nums text-muted-foreground">
                  {fmtClock(t.closed_at)}
                </td>
                <td className="max-w-[120px] truncate px-2 py-1">{kind}</td>
                <td className="px-2 py-1 uppercase">
                  {t.entry_direction ?? (t.side.toLowerCase() === 'buy' ? 'long' : '—')}
                </td>
                <td className="px-2 py-1 tabular-nums">{fmtMoney(t.entry_price)}</td>
                <td className="px-2 py-1 tabular-nums">{fmtMoney(t.exit_price)}</td>
                <td className="px-2 py-1 tabular-nums">{fmtHold(t.opened_at, t.closed_at)}</td>
                <td
                  className={cn(
                    'px-2 py-1 tabular-nums',
                    tone === 'positive' && 'text-positive',
                    tone === 'negative' && 'text-negative',
                  )}
                >
                  {fmtMoney(t.pnl_net)}
                </td>
                <td className="max-w-[90px] truncate px-2 py-1 text-muted-foreground">
                  {t.exit_trigger ? TRIGGER_LABEL[t.exit_trigger] ?? t.exit_trigger : '—'}
                </td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}

function evaluationDecisionList(payload: JudgeEvaluation['decisions']): JudgeDecisionJson[] {
  if (Array.isArray(payload)) return payload
  return payload.decisions ?? []
}

function evaluationSummaries(e: JudgeEvaluation): Array<{
  title: string
  detail: string
  tone: 'selected' | 'discarded' | 'neutral'
}> {
  const payload = e.decisions
  const rows: Array<{ title: string; detail: string; tone: 'selected' | 'discarded' | 'neutral' }> =
    []

  if (!Array.isArray(payload) && payload.selection_reason?.summary) {
    const kind = payload.selection_reason.strategy_kind
    const title = kind
      ? `${strategyGuide(kind).title} → selecionada`
      : e.selected_strategy_id
        ? `${strategyGuide(strategyKindFromInstance(e.selected_strategy_id)).title} → selecionada`
        : 'Seleção do Judge'
    rows.push({
      title,
      detail: payload.selection_reason.summary,
      tone: 'selected',
    })
  } else if (e.selected_strategy_id) {
    rows.push({
      title: `${strategyGuide(strategyKindFromInstance(e.selected_strategy_id)).title} → selecionada`,
      detail:
        (!Array.isArray(payload) && payload.regime?.summary) ||
        'Estratégia ativa nesta avaliação',
      tone: 'selected',
    })
  }

  for (const d of evaluationDecisionList(payload)) {
    if (e.selected_strategy_id && d.strategy_id === e.selected_strategy_id) continue
    if (d.state !== 'disabled' && d.state !== 'degraded') continue
    rows.push({
      title: `${strategyGuide(strategyKindFromInstance(d.strategy_id)).title} → descartada`,
      detail: judgeReasonSentence(d.reason),
      tone: 'discarded',
    })
  }

  if (rows.length === 0) {
    rows.push({
      title: 'Sem seleção',
      detail:
        (!Array.isArray(payload) && (payload.regime?.summary || payload.selection_reason?.summary)) ||
        'Avaliação sem estratégia ativa',
      tone: 'neutral',
    })
  }

  return rows
}

function JudgeTimeline({
  evaluations,
  switches,
}: {
  evaluations: JudgeEvaluation[]
  switches: StrategySwitch[]
}) {
  type Item =
    | {
        kind: 'eval'
        at: string
        title: string
        detail: string
        tone: 'selected' | 'discarded' | 'neutral'
      }
    | { kind: 'switch'; at: string; previous: string | null; next: string | null; reason: string }

  const items = useMemo(() => {
    const out: Item[] = []
    for (const s of switches.slice(0, 20)) {
      out.push({
        kind: 'switch',
        at: s.switched_at,
        previous: s.previous_strategy_id,
        next: s.new_strategy_id,
        reason: judgeReasonSentence(s.reason),
      })
    }
    for (const e of evaluations.slice(0, 24)) {
      for (const row of evaluationSummaries(e)) {
        out.push({
          kind: 'eval',
          at: e.evaluated_at,
          title: row.title,
          detail: row.detail,
          tone: row.tone,
        })
      }
    }
    out.sort((a, b) => new Date(b.at).getTime() - new Date(a.at).getTime())
    return out.slice(0, 36)
  }, [evaluations, switches])

  if (items.length === 0) {
    return <p className="font-mono text-[11px] text-muted-foreground">Sem decisões ainda.</p>
  }

  return (
    <ul className="space-y-1.5">
      {items.map((item, idx) => (
        <li key={`${item.kind}-${item.at}-${idx}`} className="border-l border-border/60 pl-2">
          <div className="font-mono text-[9px] tabular-nums text-muted-foreground">
            {fmtClock(item.at)}
          </div>
          {item.kind === 'switch' ? (
            <>
              <div className="text-[11px] leading-snug">
                <span className="text-warning">Troca</span>{' '}
                {(item.previous
                  ? strategyGuide(strategyKindFromInstance(item.previous)).title
                  : 'nenhuma') +
                  ' → ' +
                  (item.next
                    ? strategyGuide(strategyKindFromInstance(item.next)).title
                    : 'nenhuma')}
              </div>
              <p className="text-[10px] text-muted-foreground">{item.reason}</p>
            </>
          ) : (
            <>
              <div className="text-[11px] leading-snug">
                {item.tone === 'discarded' ? (
                  <span className="text-muted-foreground">{item.title}</span>
                ) : item.tone === 'selected' ? (
                  <span>
                    {item.title.split(' → ')[0]}{' '}
                    <span className="text-positive">→ selecionada</span>
                  </span>
                ) : (
                  item.title
                )}
              </div>
              <p className="line-clamp-2 text-[10px] text-muted-foreground">{item.detail}</p>
            </>
          )}
        </li>
      ))}
    </ul>
  )
}

function CandidateGrid({
  robot,
  performance,
  catalog,
}: {
  robot: OperationalRobot
  performance: StrategyPerformance[]
  catalog: StrategyCatalogEntry[]
}) {
  const fits = Array.isArray(robot.candidate_fits) ? robot.candidate_fits : []
  return (
    <div className="grid gap-1">
      {robot.candidate_kinds.map((kind) => {
        const instanceId = `${robot.id}::${kind}`
        const perf = performance.find((p) => p.strategy_id === instanceId)
        const fit = fits.find((f) => f.strategy_kind === kind || f.strategy_id === instanceId)
        const active = robot.active_strategy_id === instanceId
        const title =
          catalog.find((c) => c.kind === kind)?.display_name ?? strategyGuide(kind).title
        return (
          <div
            key={kind}
            className={cn(
              'rounded-sm border px-2 py-1.5',
              active ? 'border-accent/40 bg-accent/10' : 'border-border/40 bg-muted/10',
            )}
          >
            <div className="flex items-center gap-1.5">
              <StatusPip active={active} />
              <span className="truncate text-[11px] font-medium">{title}</span>
              {active ? (
                <Badge tone="accent" className="ml-auto h-4 px-1 text-[9px]">
                  ativa
                </Badge>
              ) : null}
            </div>
            <div className="mt-1 grid grid-cols-3 gap-1 font-mono text-[9px] text-muted-foreground">
              <span>n={perf?.total_trades ?? 0}</span>
              <span
                className={cn(
                  signTone(perf?.net_pnl) === 'negative' && 'text-negative',
                  signTone(perf?.net_pnl) === 'positive' && 'text-positive',
                )}
              >
                {fmtMoney(perf?.net_pnl ?? '0')}
              </span>
              <span>fit {fit ? fit.fit_score.toFixed(2) : '—'}</span>
            </div>
            {fit ? (
              <div className="mt-1 h-1 overflow-hidden rounded-[1px] bg-muted">
                <div
                  className="h-full bg-accent/80"
                  style={{ width: `${Math.max(4, Math.min(100, fit.fit_score * 100))}%` }}
                />
              </div>
            ) : null}
            {fit?.economic_state ? (
              <p className="mt-1 text-[9px] text-muted-foreground">
                {judgeStateLabel(fit.economic_state)}
              </p>
            ) : null}
          </div>
        )
      })}
    </div>
  )
}

function TradeDrawer({
  trade,
  robot,
  detail,
  onClose,
}: {
  trade: Trade | null
  robot: OperationalRobot | null
  detail: RobotDetail | null
  onClose: () => void
}) {
  const context = useMemo(() => {
    if (!trade || !detail) return null
    const entryEval = [...detail.evaluations]
      .sort((a, b) => new Date(b.evaluated_at).getTime() - new Date(a.evaluated_at).getTime())
      .find((e) => new Date(e.evaluated_at).getTime() <= new Date(trade.opened_at).getTime())
    const relatedSwitch = detail.switches.find((s) => {
      const t = new Date(s.switched_at).getTime()
      const open = new Date(trade.opened_at).getTime()
      const close = new Date(trade.closed_at).getTime()
      return t >= open - 1000 && t <= close + 1000
    })
    const raw = entryEval?.decisions
    const payload =
      raw && !Array.isArray(raw)
        ? {
            regime: raw.regime,
            selection_reason: raw.selection_reason,
          }
        : null
    return { entryEval, relatedSwitch, payload }
  }, [trade, detail])

  return (
    <Sheet open={!!trade} onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="max-w-lg gap-0 p-0">
        {trade ? (
          <>
            <SheetHeader className="border-b border-border/60 px-4 py-3">
              <SheetTitle className="font-mono text-sm">Trade · {trade.symbol}</SheetTitle>
              <SheetDescription className="font-mono text-[11px]">
                {fmtTime(trade.opened_at)} → {fmtTime(trade.closed_at)} ·{' '}
                {fmtHold(trade.opened_at, trade.closed_at)}
              </SheetDescription>
            </SheetHeader>
            <div className="space-y-4 overflow-y-auto px-4 py-3 text-sm">
              <div className="grid grid-cols-2 gap-3">
                <Field label="Estratégia">
                  {strategyGuide(strategyKindFromInstance(trade.strategy_id)).title}
                </Field>
                <Field label="Resultado">
                  <span
                    className={cn(
                      'font-mono',
                      signTone(trade.pnl_net) === 'positive' && 'text-positive',
                      signTone(trade.pnl_net) === 'negative' && 'text-negative',
                    )}
                  >
                    {fmtMoney(trade.pnl_net)}
                  </span>
                </Field>
                <Field label="Entrada">{fmtMoney(trade.entry_price)}</Field>
                <Field label="Saída">{fmtMoney(trade.exit_price)}</Field>
                <Field label="Sinal">
                  {trade.entry_direction
                    ? DIRECTION_LABEL[trade.entry_direction] ?? trade.entry_direction
                    : '—'}
                  {trade.entry_confidence != null
                    ? ` · conf ${trade.entry_confidence.toFixed(3)}`
                    : ''}
                </Field>
                <Field label="Motivo saída">
                  {trade.exit_reason ??
                    (trade.exit_trigger
                      ? TRIGGER_LABEL[trade.exit_trigger] ?? trade.exit_trigger
                      : '—')}
                </Field>
              </div>

              <div className="rounded-sm border border-border/50 bg-muted/20 p-2.5">
                <div className="font-mono text-[9px] uppercase tracking-[0.12em] text-muted-foreground">
                  Contexto do Judge na entrada
                </div>
                {context?.payload ? (
                  <div className="mt-1.5 space-y-1 text-[12px]">
                    <p>
                      Regime:{' '}
                      <strong>{regimeLabel(context.payload.regime?.kind ?? null)}</strong>
                      {context.payload.regime?.strength != null
                        ? ` (${Math.round(context.payload.regime.strength * 100)}%)`
                        : ''}
                    </p>
                    {context.payload.regime?.summary ? (
                      <p className="text-muted-foreground">{context.payload.regime.summary}</p>
                    ) : null}
                    {context.payload.selection_reason?.summary ? (
                      <p className="text-muted-foreground">
                        Seleção: {context.payload.selection_reason.summary}
                      </p>
                    ) : null}
                    {context.entryEval?.selected_strategy_id ? (
                      <p>
                        Selected:{' '}
                        {
                          strategyGuide(
                            strategyKindFromInstance(context.entryEval.selected_strategy_id),
                          ).title
                        }
                      </p>
                    ) : null}
                  </div>
                ) : (
                  <p className="mt-1 text-[11px] text-muted-foreground">
                    Sem avaliação do Judge no horário de abertura.
                  </p>
                )}
              </div>

              {context?.relatedSwitch ? (
                <div className="rounded-sm border border-warning/30 bg-warning/5 p-2.5 text-[12px]">
                  <div className="font-mono text-[9px] uppercase tracking-[0.12em] text-warning">
                    Troca relacionada
                  </div>
                  <p className="mt-1">
                    {switchNarrative(
                      trade.symbol,
                      context.relatedSwitch.previous_strategy_id,
                      context.relatedSwitch.new_strategy_id,
                    )}
                  </p>
                  <p className="mt-1 text-muted-foreground">
                    {judgeReasonSentence(context.relatedSwitch.reason)}
                  </p>
                </div>
              ) : null}

              <div className="grid grid-cols-3 gap-2 font-mono text-[10px] text-muted-foreground">
                <span>fees {fmtMoney(trade.fees_paid)}</span>
                <span>spread {fmtMoney(trade.spread_paid)}</span>
                <span>slip {fmtMoney(trade.slippage_paid)}</span>
              </div>

              <p className="rounded-sm border border-border/40 bg-muted/10 p-2 text-[10px] text-muted-foreground">
                Gráfico de preço ao redor da operação indisponível: OHLCV histórico não é
                persistido no Postgres (apenas latest price).
                {robot ? ` Robô: ${robot.name}.` : ''}
              </p>
            </div>
          </>
        ) : null}
      </SheetContent>
    </Sheet>
  )
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <div className="font-mono text-[9px] uppercase tracking-[0.12em] text-muted-foreground">
        {label}
      </div>
      <div className="mt-0.5 text-[12px]">{children}</div>
    </div>
  )
}

function CreateRobotDialog({
  catalog,
  busy,
  onCreate,
}: {
  catalog: StrategyCatalogEntry[]
  busy: boolean
  onCreate: Props['onCreate']
}) {
  const [open, setOpen] = useState(false)
  const [id, setId] = useState('')
  const [name, setName] = useState('')
  const [symbol, setSymbol] = useState(SYMBOLS[0]!)
  const [timeframe, setTimeframe] = useState('1m')
  const [capital, setCapital] = useState('10000')
  const [kinds, setKinds] = useState<string[]>([])
  const [err, setErr] = useState<string | null>(null)

  const toggleKind = (kind: string) => {
    setKinds((prev) => (prev.includes(kind) ? prev.filter((k) => k !== kind) : [...prev, kind]))
  }

  const submit = async () => {
    setErr(null)
    const slug =
      id.trim() ||
      name
        .trim()
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, '-')
        .replace(/^-|-$/g, '')
    try {
      await onCreate({
        id: slug,
        name: name.trim(),
        symbol,
        timeframe,
        candidate_kinds: kinds,
        paper_capital: capital,
      })
      setOpen(false)
      setId('')
      setName('')
      setKinds([])
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e))
    }
  }

  const valid = name.trim() && kinds.length > 0 && num(capital) !== null && (num(capital) ?? 0) > 0

  if (!open) {
    return (
      <Button size="sm" className="h-7 w-full text-[11px]" onClick={() => setOpen(true)}>
        Novo robô
      </Button>
    )
  }

  return (
    <div className="space-y-1.5 rounded-sm border border-border/50 bg-muted/15 p-2 text-[11px]">
      <Field label="Nome">
        <input
          className="w-full rounded-sm border border-border bg-card px-1.5 py-1"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
      </Field>
      <div className="grid grid-cols-2 gap-1.5">
        <Field label="Par">
          <select
            className="w-full rounded-sm border border-border bg-card px-1.5 py-1"
            value={symbol}
            onChange={(e) => setSymbol(e.target.value)}
          >
            {SYMBOLS.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
        </Field>
        <Field label="TF">
          <select
            className="w-full rounded-sm border border-border bg-card px-1.5 py-1"
            value={timeframe}
            onChange={(e) => setTimeframe(e.target.value)}
          >
            {TIMEFRAMES.map((t) => (
              <option key={t} value={t}>
                {t}
              </option>
            ))}
          </select>
        </Field>
      </div>
      <Field label="Capital">
        <input
          type="number"
          className="w-full rounded-sm border border-border bg-card px-1.5 py-1 font-mono"
          value={capital}
          onChange={(e) => setCapital(e.target.value)}
        />
      </Field>
      <div className="max-h-28 space-y-0.5 overflow-y-auto">
        {catalog.map((entry) => {
          const on = kinds.includes(entry.kind)
          return (
            <button
              key={entry.kind}
              type="button"
              onClick={() => toggleKind(entry.kind)}
              className={cn(
                'w-full rounded-sm border px-1.5 py-1 text-left',
                on ? 'border-accent/40 bg-accent/10' : 'border-border/40',
              )}
            >
              {entry.display_name}
            </button>
          )
        })}
      </div>
      {err ? <p className="text-negative">{err}</p> : null}
      <div className="flex gap-1">
        <Button
          size="sm"
          className="h-7 flex-1 text-[11px]"
          disabled={!valid || busy}
          onClick={() => void submit()}
        >
          Criar
        </Button>
        <Button size="sm" variant="outline" className="h-7 text-[11px]" onClick={() => setOpen(false)}>
          Cancelar
        </Button>
      </div>
    </div>
  )
}
