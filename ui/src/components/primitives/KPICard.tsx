import type { ReactNode } from 'react'
import clsx from 'clsx'

export type KpiDelta = { value: string; direction: 'up' | 'down' | 'flat' }

export interface KPICardProps {
  title: string
  value: ReactNode
  sub?: ReactNode
  delta?: KpiDelta
  chart?: ReactNode
  /** Optional accent icon shown in the top-right of the card head. */
  icon?: ReactNode
  className?: string
  mono?: boolean
}

export function KPICard({
  title,
  value,
  sub,
  delta,
  chart,
  icon,
  className,
  mono = false,
}: KPICardProps) {
  return (
    <section className={clsx('card', className)}>
      <div className="card-head">
        <p className="card-title">{title}</p>
        {icon ? <span className="dim">{icon}</span> : null}
      </div>
      <div className="kpi">
        <div className={clsx('kpi-num', mono && 'mono')}>{value}</div>
        {sub || delta ? (
          <div className="kpi-sub">
            {sub}
            {delta ? <span className={`kpi-delta ${delta.direction}`}>{delta.value}</span> : null}
          </div>
        ) : null}
      </div>
      {chart ? <div style={{ marginTop: 14 }}>{chart}</div> : null}
    </section>
  )
}
