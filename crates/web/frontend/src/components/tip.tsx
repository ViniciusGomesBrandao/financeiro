import { HelpCircle } from 'lucide-react'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/animate-ui/tooltip'

/** Span (não button) para poder ficar ao lado de cards clicáveis sem aninhar botões. */
export function Tip({ text }: { text: string }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          tabIndex={0}
          className="inline-flex h-4 w-4 shrink-0 cursor-help items-center justify-center rounded-full text-muted-foreground transition-colors hover:text-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          aria-label="Explicação"
        >
          <HelpCircle className="h-3.5 w-3.5" />
        </span>
      </TooltipTrigger>
      <TooltipContent side="top">{text}</TooltipContent>
    </Tooltip>
  )
}

export function LabelWithTip({ label, tip }: { label: string; tip?: string }) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span>{label}</span>
      {tip ? <Tip text={tip} /> : null}
    </span>
  )
}
