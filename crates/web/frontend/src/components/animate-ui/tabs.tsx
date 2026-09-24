import * as React from 'react'
import * as TabsPrimitive from '@radix-ui/react-tabs'
import { AnimatePresence, motion } from 'motion/react'
import { cn } from '@/lib/utils'

const TabsContext = React.createContext<{ value: string }>({ value: '' })

export function Tabs({
  value,
  defaultValue,
  onValueChange,
  className,
  children,
  ...props
}: React.ComponentProps<typeof TabsPrimitive.Root>) {
  const [internal, setInternal] = React.useState(defaultValue ?? '')
  const current = value ?? internal

  return (
    <TabsContext.Provider value={{ value: current }}>
      <TabsPrimitive.Root
        value={current}
        onValueChange={(v) => {
          setInternal(v)
          onValueChange?.(v)
        }}
        className={cn('w-full', className)}
        {...props}
      >
        {children}
      </TabsPrimitive.Root>
    </TabsContext.Provider>
  )
}

export function TabsList({ className, ...props }: React.ComponentProps<typeof TabsPrimitive.List>) {
  return (
    <TabsPrimitive.List
      className={cn(
        'relative flex w-full gap-1 rounded-lg border border-border bg-muted/40 p-1',
        className,
      )}
      {...props}
    />
  )
}

export function TabsTrigger({
  className,
  children,
  value,
  ...props
}: React.ComponentProps<typeof TabsPrimitive.Trigger>) {
  const ctx = React.useContext(TabsContext)
  const active = ctx.value === value

  return (
    <TabsPrimitive.Trigger
      value={value}
      className={cn(
        'relative z-10 inline-flex flex-1 items-center justify-center gap-2 rounded-md px-3 py-1.5 text-sm font-medium text-muted-foreground transition-colors',
        'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
        'data-[state=active]:text-foreground',
        className,
      )}
      {...props}
    >
      {active ? (
        <motion.span
          layoutId="main-tab-highlight"
          className="absolute inset-0 -z-10 rounded-md bg-card shadow-sm"
          transition={{ type: 'spring', stiffness: 320, damping: 28 }}
        />
      ) : null}
      <span className="relative z-10 flex items-center gap-2">{children}</span>
    </TabsPrimitive.Trigger>
  )
}

export function TabsContent({
  className,
  children,
  value,
  ...props
}: React.ComponentProps<typeof TabsPrimitive.Content>) {
  const ctx = React.useContext(TabsContext)
  const active = ctx.value === value

  return (
    <TabsPrimitive.Content
      value={value}
      forceMount
      className={cn('outline-none', !active && 'hidden', className)}
      {...props}
    >
      <AnimatePresence mode="wait">
        {active ? (
          <motion.div
            key={value}
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -4 }}
            transition={{ type: 'spring', stiffness: 300, damping: 28 }}
            className="flex h-full min-h-0 flex-col"
          >
            {children}
          </motion.div>
        ) : null}
      </AnimatePresence>
    </TabsPrimitive.Content>
  )
}
