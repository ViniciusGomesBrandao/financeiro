'use client'

import * as React from 'react'
import * as HoverCardPrimitive from '@radix-ui/react-hover-card'
import { AnimatePresence, motion, type HTMLMotionProps, type Transition } from 'motion/react'
import { cn } from '@/lib/utils'

type HoverCardContextValue = { open: boolean; setOpen: (open: boolean) => void }

const HoverCardCtx = React.createContext<HoverCardContextValue>({
  open: false,
  setOpen: () => {},
})

export function HoverCard({
  open: openProp,
  defaultOpen,
  onOpenChange,
  openDelay = 180,
  closeDelay = 120,
  ...props
}: React.ComponentProps<typeof HoverCardPrimitive.Root>) {
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
    <HoverCardCtx.Provider value={{ open, setOpen }}>
      <HoverCardPrimitive.Root
        open={open}
        onOpenChange={setOpen}
        openDelay={openDelay}
        closeDelay={closeDelay}
        {...props}
      />
    </HoverCardCtx.Provider>
  )
}

export function HoverCardTrigger(props: React.ComponentProps<typeof HoverCardPrimitive.Trigger>) {
  return <HoverCardPrimitive.Trigger data-slot="hover-card-trigger" {...props} />
}

type HoverCardContentProps = Omit<
  React.ComponentProps<typeof HoverCardPrimitive.Content>,
  'asChild' | 'forceMount'
> &
  HTMLMotionProps<'div'> & {
    transition?: Transition
  }

export function HoverCardContent({
  className,
  sideOffset = 10,
  align = 'center',
  transition = { type: 'spring', stiffness: 300, damping: 24 },
  children,
  ...props
}: HoverCardContentProps) {
  const { open } = React.useContext(HoverCardCtx)

  return (
    <AnimatePresence>
      {open ? (
        <HoverCardPrimitive.Portal forceMount>
          <HoverCardPrimitive.Content
            asChild
            forceMount
            sideOffset={sideOffset}
            align={align}
            {...props}
          >
            <motion.div
              key="hover-card-content"
              data-slot="hover-card-content"
              initial={{ opacity: 0, scale: 0.92, y: 6 }}
              animate={{ opacity: 1, scale: 1, y: 0 }}
              exit={{ opacity: 0, scale: 0.94, y: 4 }}
              transition={transition}
              className={cn(
                'z-50 w-80 rounded-xl border border-border bg-card p-4 text-sm text-foreground shadow-2xl outline-none',
                className,
              )}
            >
              {children}
            </motion.div>
          </HoverCardPrimitive.Content>
        </HoverCardPrimitive.Portal>
      ) : null}
    </AnimatePresence>
  )
}
