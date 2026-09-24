import * as React from 'react'
import type { ReactNode } from 'react'
import { cn } from '@/lib/utils'

export const Card = React.forwardRef<HTMLDivElement, React.ComponentProps<'div'>>(
  function Card({ className, ...props }, ref) {
    return (
      <div
        ref={ref}
        className={cn(
          'rounded-xl border border-border/80 bg-card text-card-foreground shadow-[0_1px_0_rgb(255_255_255/0.04)_inset]',
          className,
        )}
        {...props}
      />
    )
  },
)

export function CardHeader({ className, ...props }: React.ComponentProps<'div'>) {
  return <div className={cn('flex flex-col gap-1 p-4 pb-2', className)} {...props} />
}

export function CardTitle({ className, ...props }: React.ComponentProps<'h3'>) {
  return <h3 className={cn('text-base font-semibold tracking-tight', className)} {...props} />
}

export function CardDescription({ className, ...props }: React.ComponentProps<'p'>) {
  return <p className={cn('text-sm text-muted-foreground', className)} {...props} />
}

export function CardContent({ className, ...props }: React.ComponentProps<'div'>) {
  return <div className={cn('p-4 pt-2', className)} {...props} />
}

export function MetricTile({
  label,
  tip,
  children,
  className,
  emphasize,
}: {
  label: ReactNode
  tip?: ReactNode
  children: ReactNode
  className?: string
  emphasize?: boolean
}) {
  return (
    <div
      className={cn(
        'rounded-xl border border-border/70 bg-surface/90 p-4 transition-colors hover:border-accent/35',
        emphasize && 'border-accent/40 bg-gradient-to-br from-accent/12 via-card to-card',
        className,
      )}
    >
      <div className="mb-2 flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-[0.1em] text-muted-foreground">
        {label}
        {tip}
      </div>
      <div className="font-mono text-2xl font-semibold tracking-tight tabular-nums">{children}</div>
    </div>
  )
}
