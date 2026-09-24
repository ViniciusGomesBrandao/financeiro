import type { Price } from '@/api/types'
import { Stagger, StaggerItem } from '@/components/animate-ui/motion'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Tip } from '@/components/tip'
import { fmtMoney, fmtTime } from '@/lib/format'
import { TIPS } from '@/lib/tips'

export function PricesSection({ prices }: { prices: Price[] }) {
  return (
    <section className="space-y-3">
      <div>
        <h2 className="text-lg font-semibold tracking-tight">Preços monitorados</h2>
        <p className="text-sm text-muted-foreground">Último preço conhecido por ativo</p>
      </div>

      {prices.length === 0 ? (
        <Card>
          <CardContent className="p-8 text-center text-sm text-muted-foreground">
            Nenhum preço ainda.
          </CardContent>
        </Card>
      ) : (
        <Stagger className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
          {prices.map((p) => (
            <StaggerItem key={p.symbol}>
              <Card className="transition-colors hover:border-accent/35">
                <CardHeader className="pb-1">
                  <CardTitle className="text-base">{p.symbol}</CardTitle>
                </CardHeader>
                <CardContent>
                  <p className="font-mono text-xl font-semibold tabular-nums">
                    {fmtMoney(p.price)} <Tip text={TIPS.price} />
                  </p>
                  <p className="mt-1 text-[11px] text-muted-foreground">{fmtTime(p.updated_at)}</p>
                </CardContent>
              </Card>
            </StaggerItem>
          ))}
        </Stagger>
      )}
    </section>
  )
}
