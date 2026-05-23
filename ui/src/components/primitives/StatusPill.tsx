import clsx from 'clsx'

const TONE_FOR_STATE: Record<string, PillTone> = {
  completed: 'success',
  succeeded: 'success',
  success:   'success',
  failed:    'error',
  failure:   'error',
  error:     'error',
  running:   'info',
  pending:   'warn',
  queued:    'warn',
  timeout:   'warn',
  online:    'success',
  offline:   'error',
  stale:     'warn',
  active:    'success',
  enabled:   'success',
  paused:    'warn',
  disabled:  'outline',
  inactive:  'outline',
  unknown:   'outline',
}

export type PillTone = 'success' | 'warn' | 'error' | 'info' | 'accent' | 'outline' | 'neutral'

export interface StatusPillProps {
  state: string
  tone?: PillTone
  count?: number
  dot?: boolean
  label?: string
  className?: string
}

export function StatusPill({ state, tone, count, dot = true, label, className }: StatusPillProps) {
  const effective: PillTone = tone ?? TONE_FOR_STATE[state.toLowerCase()] ?? 'outline'
  const text = label ?? state.toLowerCase()
  return (
    <span className={clsx('pill', effective !== 'neutral' && effective, className)}>
      {dot ? <span className="dot" aria-hidden /> : null}
      {text}
      {count != null ? <span className="tnum">&nbsp;{count}</span> : null}
    </span>
  )
}
