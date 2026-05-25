import clsx from 'clsx'
import { envTone } from '@/lib/env'

export interface EnvBadgeProps {
  env: string | null | undefined
  /** Render even when envTone returns null (e.g. to debug production
   *  surfaces). Defaults to hiding the chip silently. */
  alwaysShow?: boolean
  className?: string
}

export function EnvBadge({ env, alwaysShow = false, className }: EnvBadgeProps) {
  const tone = envTone(env)
  if (!tone && !alwaysShow) return null
  if (!env) return null
  return (
    <span
      className={clsx('pill', tone ?? 'outline', className)}
      style={{ height: 20, fontSize: 10.5, letterSpacing: '0.06em', textTransform: 'uppercase' }}
      title={`CRONIQ_ENV=${env}`}
    >
      {env}
    </span>
  )
}
