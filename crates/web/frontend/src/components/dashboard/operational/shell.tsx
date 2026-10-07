import { cn } from '@/lib/utils'
import type { ReactNode } from 'react'

export function OpsShell({
  children,
  className,
}: {
  children: ReactNode
  className?: string
}) {
  return (
    <div
      className={cn(
        'flex h-full min-h-0 overflow-hidden rounded-md border border-border/70 bg-surface/80',
        className,
      )}
    >
      {children}
    </div>
  )
}

export function OpsPanel({
  title,
  right,
  children,
  className,
  bodyClassName,
}: {
  title?: string
  right?: ReactNode
  children: ReactNode
  className?: string
  bodyClassName?: string
}) {
  return (
    <section className={cn('flex min-h-0 flex-col border-border/60', className)}>
      {(title || right) && (
        <header className="flex h-7 shrink-0 items-center justify-between border-b border-border/50 px-2.5">
          {title ? (
            <h3 className="font-mono text-[10px] uppercase tracking-[0.14em] text-muted-foreground">
              {title}
            </h3>
          ) : (
            <span />
          )}
          {right}
        </header>
      )}
      <div className={cn('min-h-0 flex-1', bodyClassName)}>{children}</div>
    </section>
  )
}

export function MetricPip({
  label,
  value,
  tone = 'neutral',
  mono = true,
}: {
  label: string
  value: ReactNode
  tone?: 'positive' | 'negative' | 'neutral' | 'accent'
  mono?: boolean
}) {
  return (
    <div className="min-w-0">
      <div className="font-mono text-[9px] uppercase tracking-[0.12em] text-muted-foreground">
        {label}
      </div>
      <div
        className={cn(
          'truncate text-sm leading-tight',
          mono && 'font-mono tabular-nums',
          tone === 'positive' && 'text-positive',
          tone === 'negative' && 'text-negative',
          tone === 'accent' && 'text-accent',
          tone === 'neutral' && 'text-foreground',
        )}
      >
        {value}
      </div>
    </div>
  )
}

export function StatusPip({
  active,
  warning,
}: {
  active: boolean
  warning?: boolean
}) {
  return (
    <span
      className={cn(
        'inline-block h-1.5 w-1.5 shrink-0 rounded-[1px]',
        warning ? 'bg-warning' : active ? 'bg-positive' : 'bg-muted-foreground/50',
      )}
    />
  )
}
