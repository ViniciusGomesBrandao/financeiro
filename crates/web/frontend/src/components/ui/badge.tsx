import type { ComponentProps } from 'react'
import { cn } from '@/lib/utils'

export function Badge({
  className,
  tone = 'neutral',
  ...props
}: ComponentProps<'span'> & {
  tone?: 'neutral' | 'positive' | 'negative' | 'warning' | 'accent'
}) {
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-semibold',
        tone === 'neutral' && 'bg-muted text-muted-foreground',
        tone === 'positive' && 'bg-positive/15 text-positive',
        tone === 'negative' && 'bg-negative/15 text-negative',
        tone === 'warning' && 'bg-warning/15 text-warning',
        tone === 'accent' && 'bg-accent/15 text-accent',
        className,
      )}
      {...props}
    />
  )
}
