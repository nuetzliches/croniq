import { type ReactNode } from 'react'
import { cn } from '@/lib/utils'
import { Card, CardContent } from './card'

interface StatCardProps {
  label: string
  value: ReactNode
  sub?: ReactNode
  icon?: ReactNode
  trend?: 'up' | 'down' | 'neutral'
  className?: string
  href?: string
}

export function StatCard({ label, value, sub, icon, className }: StatCardProps) {
  return (
    <Card className={cn('', className)}>
      <CardContent className="pt-4">
        <div className="flex items-start justify-between">
          <div className="space-y-1">
            <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide">{label}</p>
            <p className="text-2xl font-bold text-foreground">{value}</p>
            {sub && <p className="text-xs text-muted-foreground">{sub}</p>}
          </div>
          {icon && (
            <div className="rounded-md bg-primary/10 p-2 text-primary">
              {icon}
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  )
}
