import { type HTMLAttributes } from 'react'
import { cn } from '@/lib/utils'

export type BadgeVariant = 'ok' | 'err' | 'warn' | 'info' | 'neutral'

interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  variant?: BadgeVariant
}

const variantClasses: Record<BadgeVariant, string> = {
  ok: 'bg-status-ok-bg text-status-ok-fg',
  err: 'bg-status-err-bg text-status-err-fg',
  warn: 'bg-status-warn-bg text-status-warn-fg',
  info: 'bg-status-info-bg text-status-info-fg',
  neutral: 'bg-status-neutral-bg text-status-neutral-fg',
}

export function Badge({ variant = 'neutral', className, children, ...props }: BadgeProps) {
  return (
    <span
      role="status"
      className={cn(
        'inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium',
        variantClasses[variant],
        className
      )}
      {...props}
    >
      {children}
    </span>
  )
}

export function stateVariant(state: string): BadgeVariant {
  switch (state.toLowerCase()) {
    case 'completed': return 'ok'
    case 'failed':
    case 'dead': return 'err'
    case 'claimed': return 'info'
    case 'queued': return 'warn'
    case 'cancelled': return 'neutral'
    default: return 'neutral'
  }
}
