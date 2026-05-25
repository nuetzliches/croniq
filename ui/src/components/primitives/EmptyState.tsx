import type { LucideIcon } from 'lucide-react'
import type { ReactNode } from 'react'

export interface EmptyStateProps {
  icon?: LucideIcon
  title: string
  desc?: string
  actions?: ReactNode
}

export function EmptyState({ icon: Icon, title, desc, actions }: EmptyStateProps) {
  return (
    <div className="empty-state">
      {Icon ? (
        <div className="empty-state-icon">
          <Icon size={22} />
        </div>
      ) : null}
      <div className="empty-state-title">{title}</div>
      {desc ? <div className="empty-state-desc">{desc}</div> : null}
      {actions ? <div className="empty-state-actions">{actions}</div> : null}
    </div>
  )
}
