import { REFRESH_MS } from '@/hooks/use-dashboard'
import { fmtTime } from '@/lib/format'
import { PulseDot } from '@/components/animate-ui/motion'
import { Tip } from '@/components/tip'
import { TIPS } from '@/lib/tips'

export function DashboardHeader({
  asOf,
  error,
}: {
  asOf: string | null | undefined
  error: string | null
}) {
  return (
    <header className="shrink-0 border-b border-border bg-background/95">
      <div className="flex h-11 items-center justify-between gap-3 px-2.5 sm:px-3">
        <div className="flex min-w-0 items-center gap-2 text-sm">
          <PulseDot />
          <span className="font-medium">Paper trading</span>
          <Tip text={TIPS.paperTrading} />
          <span className="hidden text-muted-foreground md:inline">· simulação</span>
        </div>
        <p className="shrink-0 text-[11px] text-muted-foreground">
          {error
            ? `erro: ${error}`
            : asOf
              ? `${fmtTime(asOf)} · ${REFRESH_MS / 1000}s`
              : 'aguardando…'}
        </p>
      </div>
    </header>
  )
}
