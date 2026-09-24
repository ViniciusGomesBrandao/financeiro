import { useEffect, useState, type ReactNode } from 'react'
import { Bot, Play, Square } from 'lucide-react'
import { fetchRobotDetail } from '@/api/client'
import type {
  JudgeDecisionJson,
  OperationalData,
  OperationalRobot,
  RobotDetail,
  StrategyCatalogEntry,
} from '@/api/types'
import { FadeIn, Stagger, StaggerItem } from '@/components/animate-ui/motion'
import { PlainSentence } from '@/components/narrative/inline'
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '@/components/ui/accordion'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/tip'
import { fmtMoney, fmtPct, fmtTime, num, signTone } from '@/lib/format'
import {
  expectancySentence,
  judgeMoodLabel,
  judgeReasonSentence,
  judgeStateLabel,
  regimeLabel,
  robotHeadline,
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
    const id = window.setInterval(() => void load(), 5000)
    return () => {
      cancelled = true
      window.clearInterval(id)
    }
  }, [selected?.id, selected?.status, selected?.updated_at])

  return (
    <FadeIn className="flex h-full min-h-0 flex-col gap-2 overflow-hidden">
      <div className="grid min-h-0 flex-1 gap-2 lg:grid-cols-12">
        <Panel title="Robôs" subtitle={`${data.robots.length}`} className="lg:col-span-3">
          <div className="border-b border-border/50 p-2">
            <CreateRobotDialog catalog={data.catalog} busy={busy} onCreate={onCreate} />
          </div>
          <ul className="min-h-0 flex-1 overflow-y-auto p-1.5">
            {data.robots.length === 0 ? (
              <li className="p-3 text-sm text-muted-foreground">
                Nenhum robô ainda. Crie um para operar em paper trading.
              </li>
            ) : (
              <Stagger className="grid gap-1">
                {data.robots.map((robot) => (
                  <StaggerItem key={robot.id}>
                    <RobotListItem
                      robot={robot}
                      active={robot.id === selected?.id}
                      onSelect={() => setSelectedId(robot.id)}
                    />
                  </StaggerItem>
                ))}
              </Stagger>
            )}
          </ul>
        </Panel>

        <div className="flex min-h-0 flex-col gap-2 overflow-y-auto lg:col-span-9">
          {selected ? (
            <RobotOperationalView
              robot={detail?.robot ?? selected}
              detail={detail}
              busy={busy}
              onToggle={onToggle}
            />
          ) : (
            <Panel title="Visão do robô" className="flex-1">
              <p className="p-4 text-sm text-muted-foreground">
                Selecione ou crie um robô para ver o estado operacional.
              </p>
            </Panel>
          )}
        </div>
      </div>
    </FadeIn>
  )
}

function RobotListItem({
  robot,
  active,
  onSelect,
}: {
  robot: OperationalRobot
  active: boolean
  onSelect: () => void
}) {
  const tone = signTone(robot.net_pnl)
  return (
    <button
      type="button"
      onClick={onSelect}
      className={cn(
        'flex w-full items-center gap-2 rounded-md border px-2.5 py-2 text-left text-xs transition-colors',
        active ? 'border-accent/50 bg-accent/10' : 'border-border/50 bg-card/40 hover:bg-muted/40',
      )}
    >
      <Bot className="h-3.5 w-3.5 shrink-0 text-accent" />
      <div className="min-w-0 flex-1">
        <p className="truncate font-semibold">{robot.name}</p>
        <p className="truncate text-[10px] text-muted-foreground">
          {robot.symbol} · {robot.status === 'running' ? 'ativo' : 'parado'}
        </p>
      </div>
      <span
        className={cn(
          'shrink-0 font-mono text-[10px] font-semibold',
          tone === 'positive' && 'text-positive',
          tone === 'negative' && 'text-negative',
        )}
      >
        {fmtMoney(robot.net_pnl)}
      </span>
    </button>
  )
}

function RobotOperationalView({
  robot,
  detail,
  busy,
  onToggle,
}: {
  robot: OperationalRobot
  detail: RobotDetail | null
  busy: boolean
  onToggle: (id: string, status: 'running' | 'stopped') => Promise<void>
}) {
  const running = robot.status === 'running'
  const activeKind = robot.active_strategy_id
    ? strategyGuide(strategyKindFromInstance(robot.active_strategy_id))
    : null
  const pnlTone = signTone(robot.net_pnl)
  const moodTone =
    robot.judge_mood === 'satisfied'
      ? 'positive'
      : robot.judge_mood === 'looking'
        ? 'warning'
        : 'neutral'

  return (
    <>
      <Panel title="Agora" tip="Primeiro olhar: o essencial do robô, sem números demais.">
        <div className="space-y-3 p-3">
          <div className="flex flex-wrap items-center gap-2">
            <Badge tone={running ? 'accent' : 'neutral'}>
              {running ? 'Rodando (paper)' : 'Parado'}
            </Badge>
            <Badge tone="neutral">{robot.symbol}</Badge>
            <Badge tone="neutral">{robot.timeframe}</Badge>
            <Badge tone={moodTone}>{judgeMoodLabel(robot.judge_mood)}</Badge>
            {robot.market_regime ? (
              <Badge tone="neutral">{regimeLabel(robot.market_regime)}</Badge>
            ) : null}
          </div>

          <PlainSentence className="text-sm leading-relaxed">{robotHeadline(robot)}</PlainSentence>

          <div className="grid gap-2 sm:grid-cols-2">
            <MicroCard
              label="Estratégia ativa"
              value={activeKind?.title ?? 'Nenhuma ainda'}
              hint={
                activeKind
                  ? 'É a regra que o robô está autorizado a usar neste mercado agora.'
                  : 'O Judge ainda não escolheu uma vencedora entre as candidatas.'
              }
            />
            <MicroCard
              label="Por quê"
              value={robot.active_why}
              hint="Resumo do veredito do Strategy Judge — sem recalcular no frontend."
            />
            <MicroCard
              label="Regime do mercado"
              value={
                robot.market_regime
                  ? `${regimeLabel(robot.market_regime)}${
                      robot.regime_strength != null
                        ? ` · força ${(robot.regime_strength * 100).toFixed(0)}%`
                        : ''
                    }`
                  : 'Aguardando features'
              }
              hint={
                robot.regime_summary ??
                'Classificação determinística do comportamento recente (não é probabilidade).'
              }
            />
            <MicroCard
              label="Resultado"
              value={
                robot.equity
                  ? `patrimônio ${fmtMoney(robot.equity)}`
                  : fmtMoney(robot.net_pnl)
              }
              hint="Equity isolada deste robô: caixa + valor das posições abertas. O P&L líquido abaixo conta só trades fechados deste robô."
              tone={pnlTone === 'neutral' ? undefined : pnlTone}
            />
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <Button
              size="sm"
              variant={running ? 'outline' : 'default'}
              disabled={busy}
              onClick={() => void onToggle(robot.id, running ? 'stopped' : 'running')}
            >
              {running ? (
                <>
                  <Square className="h-3.5 w-3.5" /> Parar
                </>
              ) : (
                <>
                  <Play className="h-3.5 w-3.5" /> Iniciar
                </>
              )}
            </Button>
            {robot.engine_restart_required && running ? (
              <span className="text-[10px] text-warning">
                Se este robô é novo, reinicie o quant-engine uma vez para carregá-lo. Parar já
                bloqueia novas compras sem restart.
              </span>
            ) : null}
          </div>
        </div>
      </Panel>

      <Panel title="Detalhes" tip="Camada secundária: abra só o que precisar auditar.">
        <div className="px-3">
          <Accordion type="multiple" className="w-full">
            <AccordionItem value="compare">
              <AccordionTrigger>Comparação das estratégias</AccordionTrigger>
              <AccordionContent>
                <CandidateCompare detail={detail} robot={robot} />
              </AccordionContent>
            </AccordionItem>
            <AccordionItem value="metrics">
              <AccordionTrigger>Métricas completas</AccordionTrigger>
              <AccordionContent>
                <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
                  <Fact label="Caixa" value={robot.cash ? fmtMoney(robot.cash) : fmtMoney(robot.paper_capital)} />
                  <Fact
                    label="Patrimônio (equity)"
                    value={robot.equity ? fmtMoney(robot.equity) : '—'}
                    tone={
                      robot.return_pct
                        ? (() => {
                            const t = signTone(robot.return_pct)
                            return t === 'neutral' ? undefined : t
                          })()
                        : undefined
                    }
                  />
                  <Fact
                    label="Retorno s/ capital"
                    value={
                      robot.return_pct != null
                        ? `${(Number(robot.return_pct) * 100).toFixed(2)}%`
                        : '—'
                    }
                  />
                  <Fact label="Capital inicial" value={fmtMoney(robot.paper_capital)} />
                  <Fact label="P&amp;L líquido (fechados)" value={fmtMoney(robot.net_pnl)} tone={pnlTone === 'neutral' ? undefined : pnlTone} />
                  <Fact label="Taxa de acerto" value={fmtPct(robot.win_rate)} />
                  <Fact
                    label="Profit factor"
                    value={
                      robot.profit_factor
                        ? num(robot.profit_factor)?.toFixed(2) ?? '—'
                        : '—'
                    }
                  />
                  <Fact label="Expectancy" value={fmtPct(robot.expectancy)} />
                  <Fact label="Drawdown máx." value={fmtMoney(robot.max_drawdown)} />
                  <Fact label="Trades" value={String(robot.closed_trades_count)} />
                </div>
                <p className="mt-2 text-xs text-muted-foreground">
                  {expectancySentence(robot.expectancy)}
                </p>
              </AccordionContent>
            </AccordionItem>
            <AccordionItem value="history">
              <AccordionTrigger>Histórico de decisões do Judge</AccordionTrigger>
              <AccordionContent>
                <DecisionHistory detail={detail} />
              </AccordionContent>
            </AccordionItem>
            <AccordionItem value="trades">
              <AccordionTrigger>Trades individuais</AccordionTrigger>
              <AccordionContent>
                <TradesList detail={detail} />
              </AccordionContent>
            </AccordionItem>
            <AccordionItem value="curve">
              <AccordionTrigger>Resultado acumulado</AccordionTrigger>
              <AccordionContent>
                <PnlCurve detail={detail} />
              </AccordionContent>
            </AccordionItem>
            <AccordionItem value="judge">
              <AccordionTrigger>Detalhes técnicos do Judge</AccordionTrigger>
              <AccordionContent>
                <JudgeTech detail={detail} robot={robot} />
              </AccordionContent>
            </AccordionItem>
            <AccordionItem value="switches">
              <AccordionTrigger>Trocas de estratégia</AccordionTrigger>
              <AccordionContent>
                <SwitchesList detail={detail} />
              </AccordionContent>
            </AccordionItem>
          </Accordion>
        </div>
      </Panel>
    </>
  )
}

function CandidateCompare({
  detail,
  robot,
}: {
  detail: RobotDetail | null
  robot: OperationalRobot
}) {
  const fits = robot.candidate_fits ?? []
  if (!detail && fits.length === 0) {
    return <p className="text-xs text-muted-foreground">Carregando comparação…</p>
  }

  if (fits.length > 0) {
    const sorted = [...fits].sort((a, b) => b.fit_score - a.fit_score)
    return (
      <ul className="space-y-2">
        {sorted.map((fit) => {
          const title = strategyGuide(fit.strategy_kind).title
          const selected = robot.active_strategy_id === fit.strategy_id
          const perf = detail?.candidate_performance.find((p) => p.strategy_id === fit.strategy_id)
          return (
            <li
              key={fit.strategy_id}
              className={cn(
                'rounded-md border px-2.5 py-2 text-xs',
                selected ? 'border-accent/40 bg-accent/10' : 'border-border/50',
              )}
            >
              <div className="flex items-center justify-between gap-2">
                <span className="font-semibold">
                  {title}
                  {selected ? ' · ativa' : ''}
                </span>
                <span className="font-mono text-muted-foreground">
                  afinidade {(fit.fit_score * 100).toFixed(0)}%
                </span>
              </div>
              <p className="mt-0.5 text-[10px] text-muted-foreground">
                estado econômico: {judgeStateLabel(fit.economic_state)}
                {perf
                  ? ` · ${perf.total_trades} trades · ${fmtMoney(perf.net_pnl)}`
                  : ' · ainda sem trades'}
              </p>
            </li>
          )
        })}
      </ul>
    )
  }

  if (!detail || detail.candidate_performance.length === 0) {
    return (
      <p className="text-xs text-muted-foreground">
        Ainda sem avaliação de regime nem trades por candidata.
      </p>
    )
  }
  return (
    <ul className="space-y-2">
      {detail.candidate_performance.map((row) => {
        const title = strategyGuide(strategyKindFromInstance(row.strategy_id)).title
        const selected = robot.active_strategy_id === row.strategy_id
        return (
          <li
            key={row.strategy_id}
            className={cn(
              'rounded-md border px-2.5 py-2 text-xs',
              selected ? 'border-accent/40 bg-accent/10' : 'border-border/50',
            )}
          >
            <div className="flex items-center justify-between gap-2">
              <span className="font-semibold">
                {title}
                {selected ? ' · ativa' : ''}
              </span>
              <span className={cn('font-mono font-semibold', signTone(row.net_pnl) === 'positive' && 'text-positive', signTone(row.net_pnl) === 'negative' && 'text-negative')}>
                {fmtMoney(row.net_pnl)}
              </span>
            </div>
            <p className="mt-0.5 text-[10px] text-muted-foreground">
              {row.total_trades} trades · acerto {fmtPct(row.win_rate)}
              {row.profit_factor ? ` · PF ${num(row.profit_factor)?.toFixed(2)}` : ''}
            </p>
          </li>
        )
      })}
    </ul>
  )
}

function DecisionHistory({ detail }: { detail: RobotDetail | null }) {
  if (!detail?.evaluations.length) {
    return <p className="text-xs text-muted-foreground">Nenhuma avaliação persistida ainda.</p>
  }
  return (
    <ul className="max-h-56 space-y-2 overflow-y-auto">
      {detail.evaluations.slice(0, 12).map((e) => (
        <li key={e.id} className="rounded-md bg-muted/30 px-2.5 py-2 text-xs">
          <p className="text-[10px] text-muted-foreground">{fmtTime(e.evaluated_at)}</p>
          <p>
            Selecionada:{' '}
            {e.selected_strategy_id
              ? strategyGuide(strategyKindFromInstance(e.selected_strategy_id)).title
              : 'nenhuma'}
          </p>
        </li>
      ))}
    </ul>
  )
}

function TradesList({ detail }: { detail: RobotDetail | null }) {
  if (!detail?.trades.length) {
    return <p className="text-xs text-muted-foreground">Nenhum trade fechado deste robô.</p>
  }
  return (
    <ul className="max-h-56 divide-y divide-border/40 overflow-y-auto">
      {detail.trades.slice(0, 30).map((t, i) => {
        const tone = signTone(t.pnl_net)
        return (
          <li key={`${t.closed_at}-${i}`} className="flex items-center gap-2 py-1.5 text-xs">
            <span className="truncate text-muted-foreground">
              {strategyGuide(strategyKindFromInstance(t.strategy_id)).title}
            </span>
            <span className="ml-auto text-[10px] text-muted-foreground">{fmtTime(t.closed_at)}</span>
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

function PnlCurve({ detail }: { detail: RobotDetail | null }) {
  const points = detail?.realized_pnl_curve ?? []
  if (points.length === 0) {
    return (
      <p className="text-xs text-muted-foreground">
        Sem curva ainda. Isto mostra o resultado acumulado dos trades deste robô — não o patrimônio
        global da conta.
      </p>
    )
  }
  const values = points.map((p) => num(p.cumulative_pnl) ?? 0)
  const min = Math.min(0, ...values)
  const max = Math.max(0, ...values)
  const span = Math.max(max - min, 1e-9)
  const last = points[points.length - 1]!
  return (
    <div className="space-y-2">
      <p className="text-xs text-muted-foreground">
        Resultado acumulado ao longo dos trades fechados. Último ponto:{' '}
        <span className="font-mono font-semibold text-foreground">{fmtMoney(last.cumulative_pnl)}</span>
      </p>
      <div className="flex h-16 items-end gap-px rounded-md bg-muted/30 p-1">
        {points.slice(-48).map((p, i) => {
          const v = num(p.cumulative_pnl) ?? 0
          const h = ((v - min) / span) * 100
          return (
            <div
              key={`${p.at}-${i}`}
              className={cn('min-w-[2px] flex-1 rounded-sm', v >= 0 ? 'bg-positive/70' : 'bg-negative/70')}
              style={{ height: `${Math.max(h, 4)}%` }}
              title={`${fmtTime(p.at)} · ${fmtMoney(p.cumulative_pnl)}`}
            />
          )
        })}
      </div>
    </div>
  )
}

function JudgeTech({
  detail,
  robot,
}: {
  detail: RobotDetail | null
  robot: OperationalRobot
}) {
  const latest = detail?.evaluations[0]
  const decisions = Array.isArray(latest?.decisions) ? (latest!.decisions as JudgeDecisionJson[]) : []
  const filtered = decisions.filter((d) => robot.strategy_instance_ids.includes(d.strategy_id))
  if (filtered.length === 0) {
    return <p className="text-xs text-muted-foreground">Sem decisão técnica recente.</p>
  }
  return (
    <ul className="space-y-2">
      {filtered.map((d) => (
        <li key={d.strategy_id} className="rounded-md border border-border/50 px-2.5 py-2 text-xs">
          <div className="flex items-center gap-2">
            <span className="font-semibold">
              {strategyGuide(strategyKindFromInstance(d.strategy_id)).title}
            </span>
            <Badge
              tone={
                d.state === 'active' ? 'positive' : d.state === 'degraded' ? 'warning' : 'neutral'
              }
            >
              {judgeStateLabel(d.state)}
            </Badge>
          </div>
          <p className="mt-1 text-muted-foreground">{judgeReasonSentence(d.reason)}</p>
          <p className="mt-1 text-[10px] text-muted-foreground">
            {expectancySentence(d.metrics.expectancy)} · amostra {d.metrics.trades} · win{' '}
            {fmtPct(d.metrics.win_rate)}
          </p>
        </li>
      ))}
    </ul>
  )
}

function SwitchesList({ detail }: { detail: RobotDetail | null }) {
  if (!detail?.switches.length) {
    return <p className="text-xs text-muted-foreground">Nenhuma troca registrada.</p>
  }
  return (
    <ul className="space-y-2">
      {detail.switches.map((s) => (
        <li key={s.id} className="rounded-md bg-muted/30 px-2.5 py-2 text-xs">
          <p>{switchNarrative(s.symbol, s.previous_strategy_id, s.new_strategy_id)}</p>
          <p className="mt-0.5 text-[10px] text-muted-foreground">
            {fmtTime(s.switched_at)} · {judgeReasonSentence(s.reason)}
          </p>
        </li>
      ))}
    </ul>
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
    setKinds((prev) =>
      prev.includes(kind) ? prev.filter((k) => k !== kind) : [...prev, kind],
    )
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
      <Button size="sm" className="w-full" onClick={() => setOpen(true)}>
        Novo robô
      </Button>
    )
  }

  return (
    <div className="space-y-2 rounded-md border border-border/60 bg-muted/20 p-2 text-sm">
      <p className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Criar robô</p>
      <Field label="Nome">
        <input
          className="w-full rounded-md border border-border bg-card px-2 py-1.5 text-sm"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="BTC conservador"
        />
      </Field>
      <Field label="ID (opcional)">
        <input
          className="w-full rounded-md border border-border bg-card px-2 py-1.5 font-mono text-xs"
          value={id}
          onChange={(e) => setId(e.target.value)}
        />
      </Field>
      <div className="grid grid-cols-2 gap-2">
        <Field label="Símbolo">
          <select
            className="w-full rounded-md border border-border bg-card px-2 py-1.5 text-sm"
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
        <Field label="Timeframe">
          <select
            className="w-full rounded-md border border-border bg-card px-2 py-1.5 text-sm"
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
      <Field label="Capital fictício (referência)">
        <input
          type="number"
          className="w-full rounded-md border border-border bg-card px-2 py-1.5 font-mono text-sm"
          value={capital}
          onChange={(e) => setCapital(e.target.value)}
        />
      </Field>
      <div>
        <p className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
          Estratégias candidatas
        </p>
        <div className="grid max-h-36 gap-1 overflow-y-auto">
          {catalog.map((entry) => {
            const on = kinds.includes(entry.kind)
            return (
              <button
                key={entry.kind}
                type="button"
                onClick={() => toggleKind(entry.kind)}
                className={cn(
                  'rounded-md border px-2 py-1.5 text-left text-xs',
                  on ? 'border-accent/50 bg-accent/10' : 'border-border/50 hover:bg-muted/40',
                )}
              >
                <span className="font-semibold">{entry.display_name}</span>
              </button>
            )
          })}
        </div>
      </div>
      {err ? <p className="text-[11px] text-negative">{err}</p> : null}
      <div className="flex gap-2">
        <Button size="sm" className="flex-1" disabled={!valid || busy} onClick={() => void submit()}>
          Criar
        </Button>
        <Button size="sm" variant="outline" onClick={() => setOpen(false)}>
          Cancelar
        </Button>
      </div>
    </div>
  )
}

function MicroCard({
  label,
  value,
  hint,
  tone,
}: {
  label: string
  value: string
  hint: string
  tone?: 'positive' | 'negative'
}) {
  return (
    <div className="rounded-md border border-border/50 bg-muted/25 px-2.5 py-2">
      <p className="inline-flex items-center gap-1 text-[10px] uppercase tracking-wide text-muted-foreground">
        {label}
        <Tip text={hint} />
      </p>
      <p
        className={cn(
          'mt-0.5 text-sm font-medium leading-snug',
          tone === 'positive' && 'text-positive',
          tone === 'negative' && 'text-negative',
        )}
      >
        {value}
      </p>
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

function Fact({
  label,
  value,
  tone,
}: {
  label: string
  value: string
  tone?: 'positive' | 'negative'
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

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="block">
      <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
        {label}
      </span>
      {children}
    </label>
  )
}
