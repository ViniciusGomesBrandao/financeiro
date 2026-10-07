import { motion } from 'motion/react'
import { num } from '@/lib/format'
import { cn } from '@/lib/utils'

type Point = { at: string; value: number }

export function MiniSeries({
  points,
  className,
  strokeClass = 'stroke-accent',
  fillClass = 'fill-accent/15',
}: {
  points: Point[]
  className?: string
  strokeClass?: string
  fillClass?: string
}) {
  const values = points.map((p) => p.value).filter((v) => Number.isFinite(v))
  if (values.length < 2) {
    return (
      <div
        className={cn(
          'flex h-full items-center justify-center font-mono text-[10px] text-muted-foreground',
          className,
        )}
      >
        Sem série ainda
      </div>
    )
  }

  const min = Math.min(...values)
  const max = Math.max(...values)
  const span = max - min || 1
  const w = 100
  const h = 36
  const coords = values.map((v, i) => {
    const x = (i / (values.length - 1)) * w
    const y = h - ((v - min) / span) * (h - 4) - 2
    return `${x.toFixed(2)},${y.toFixed(2)}`
  })
  const line = coords.join(' ')
  const area = `0,${h} ${line} ${w},${h}`

  return (
    <svg viewBox={`0 0 ${w} ${h}`} className={cn('h-full w-full', className)} preserveAspectRatio="none">
      <motion.polygon
        points={area}
        className={fillClass}
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ duration: 0.35 }}
      />
      <motion.polyline
        points={line}
        fill="none"
        className={cn(strokeClass, 'stroke-[1.2]')}
        strokeLinejoin="round"
        strokeLinecap="round"
        initial={{ pathLength: 0, opacity: 0.4 }}
        animate={{ pathLength: 1, opacity: 1 }}
        transition={{ duration: 0.5, ease: [0.22, 1, 0.36, 1] }}
      />
    </svg>
  )
}

export function toSeries(
  rows: Array<{ at: string; equity?: string; cumulative_pnl?: string }>,
  key: 'equity' | 'cumulative_pnl',
): Point[] {
  return rows
    .map((r) => ({ at: r.at, value: num(key === 'equity' ? r.equity : r.cumulative_pnl) ?? NaN }))
    .filter((p) => Number.isFinite(p.value))
}
