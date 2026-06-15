import { type HTMLAttributes } from 'react'
import { cn } from '@/lib/utils'

export type BadgeVariant = 'ok' | 'err' | 'warn' | 'info' | 'neutral'

interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  variant?: BadgeVariant
}

// Each variant carries a tone-matched border so the badge keeps a defined
// edge at rest — the pale fills alone washed out against a white card and
// only became legible once a row hover darkened the backdrop.
const variantClasses: Record<BadgeVariant, string> = {
  ok: 'bg-status-ok-bg text-status-ok-fg border border-status-ok-fg/30',
  err: 'bg-status-err-bg text-status-err-fg border border-status-err-fg/30',
  warn: 'bg-status-warn-bg text-status-warn-fg border border-status-warn-fg/30',
  info: 'bg-status-info-bg text-status-info-fg border border-status-info-fg/30',
  neutral: 'bg-status-neutral-bg text-status-neutral-fg border border-border',
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
