'use client'

import * as React from 'react'
import * as TooltipPrimitive from '@radix-ui/react-tooltip'
import { AnimatePresence, motion, type HTMLMotionProps, type Transition } from 'motion/react'
import { cn } from '@/lib/utils'

type TooltipContextValue = { open: boolean; setOpen: (open: boolean) => void }

const TooltipCtx = React.createContext<TooltipContextValue>({
  open: false,
  setOpen: () => {},
})

export function TooltipProvider({
  delayDuration = 120,
  ...props
}: React.ComponentProps<typeof TooltipPrimitive.Provider>) {
  return <TooltipPrimitive.Provider delayDuration={delayDuration} {...props} />
}

export function Tooltip({
  open: openProp,
  defaultOpen,
  onOpenChange,
  ...props
}: React.ComponentProps<typeof TooltipPrimitive.Root>) {
  const [uncontrolled, setUncontrolled] = React.useState(defaultOpen ?? false)
  const open = openProp ?? uncontrolled
  const setOpen = React.useCallback(
    (next: boolean) => {
      setUncontrolled(next)
      onOpenChange?.(next)
    },
    [onOpenChange],
  )

  return (
    <TooltipCtx.Provider value={{ open, setOpen }}>
      <TooltipPrimitive.Root open={open} onOpenChange={setOpen} {...props} />
    </TooltipCtx.Provider>
  )
}

export function TooltipTrigger(props: React.ComponentProps<typeof TooltipPrimitive.Trigger>) {
  return <TooltipPrimitive.Trigger data-slot="tooltip-trigger" {...props} />
}

type TooltipContentProps = Omit<
  React.ComponentProps<typeof TooltipPrimitive.Content>,
  'asChild' | 'forceMount'
> &
  HTMLMotionProps<'div'> & {
    transition?: Transition
  }

export function TooltipContent({
  className,
  sideOffset = 8,
  transition = { type: 'spring', stiffness: 320, damping: 24 },
  children,
  ...props
}: TooltipContentProps) {
  const { open } = React.useContext(TooltipCtx)

  return (
    <AnimatePresence>
      {open ? (
        <TooltipPrimitive.Portal forceMount>
          <TooltipPrimitive.Content asChild forceMount sideOffset={sideOffset} {...props}>
            <motion.div
              key="tooltip-content"
              data-slot="tooltip-content"
              initial={{ opacity: 0, scale: 0.86, y: 4 }}
              animate={{ opacity: 1, scale: 1, y: 0 }}
              exit={{ opacity: 0, scale: 0.9, y: 2 }}
              transition={transition}
              className={cn(
                'z-50 max-w-xs rounded-lg border border-border bg-card px-3 py-2 text-xs leading-relaxed text-foreground shadow-xl',
                className,
              )}
            >
              {children}
              <TooltipPrimitive.Arrow className="fill-card" />
            </motion.div>
          </TooltipPrimitive.Content>
        </TooltipPrimitive.Portal>
      ) : null}
    </AnimatePresence>
  )
}
