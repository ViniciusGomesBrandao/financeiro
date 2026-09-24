import { Activity, Briefcase, Bot, LayoutDashboard } from 'lucide-react'
import { DashboardHeader } from '@/components/dashboard/header'
import { WorkspaceOperacional } from '@/components/dashboard/operational-workspace'
import {
  WorkspaceLog,
  WorkspaceOperacoes,
  WorkspaceResumo,
} from '@/components/dashboard/workspace'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/animate-ui/tabs'
import { FadeIn } from '@/components/animate-ui/motion'
import { TooltipProvider } from '@/components/animate-ui/tooltip'
import { useDashboard } from '@/hooks/use-dashboard'
import { useOperational } from '@/hooks/use-operational'

const NAV = [
  { value: 'resumo', label: 'Resumo', icon: LayoutDashboard },
  { value: 'operacional', label: 'Operacional', icon: Bot },
  { value: 'operacoes', label: 'Operações', icon: Briefcase },
  { value: 'log', label: 'Log', icon: Activity },
] as const

export default function App() {
  const { data, error, loading } = useDashboard()
  const operational = useOperational()

  return (
    <TooltipProvider>
      <div className="flex h-dvh flex-col overflow-hidden bg-background">
        <DashboardHeader
          asOf={data?.overview.as_of}
          error={error ?? operational.error}
        />

        {loading && !data && operational.loading ? (
          <FadeIn className="p-3">
            <p className="text-sm text-muted-foreground">Carregando…</p>
          </FadeIn>
        ) : data || operational.data ? (
          <Tabs defaultValue="resumo" className="flex min-h-0 flex-1 flex-col px-2 pb-2 pt-1.5 sm:px-2.5">
            <TabsList aria-label="Navegação" className="mb-1.5 max-w-sm shrink-0">
              {NAV.map(({ value, label, icon: Icon }) => (
                <TabsTrigger key={value} value={value}>
                  <Icon className="h-3.5 w-3.5 opacity-80" />
                  {label}
                </TabsTrigger>
              ))}
            </TabsList>

            <TabsContent value="resumo" className="mt-0 min-h-0 flex-1">
              {data ? (
                <WorkspaceResumo data={data} />
              ) : (
                <p className="p-3 text-sm text-muted-foreground">Resumo indisponível.</p>
              )}
            </TabsContent>
            <TabsContent value="operacional" className="mt-0 min-h-0 flex-1">
              {operational.data ? (
                <WorkspaceOperacional
                  data={operational.data}
                  busy={operational.busy}
                  onCreate={operational.create}
                  onToggle={operational.toggleStatus}
                />
              ) : operational.loading ? (
                <FadeIn className="p-3">
                  <p className="text-sm text-muted-foreground">Carregando operacional…</p>
                </FadeIn>
              ) : (
                <p className="p-3 text-sm text-negative">Não foi possível carregar os robôs.</p>
              )}
            </TabsContent>
            <TabsContent value="operacoes" className="mt-0 min-h-0 flex-1">
              {data ? (
                <WorkspaceOperacoes data={data} />
              ) : (
                <p className="p-3 text-sm text-muted-foreground">Operações indisponíveis.</p>
              )}
            </TabsContent>
            <TabsContent value="log" className="mt-0 min-h-0 flex-1">
              {data ? (
                <WorkspaceLog data={data} />
              ) : (
                <p className="p-3 text-sm text-muted-foreground">Log indisponível.</p>
              )}
            </TabsContent>
          </Tabs>
        ) : (
          <p className="p-3 text-sm text-negative">Não foi possível carregar os dados.</p>
        )}
      </div>
    </TooltipProvider>
  )
}
