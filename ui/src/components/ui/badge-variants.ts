import type { BadgeVariant } from './badge'

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
