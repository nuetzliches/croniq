/** Returns the pill tone for a CRONIQ_ENV value, or `null` when the
 *  environment is `production` / `unknown`. Kept in sync with the
 *  guidance in docs/operations.md — production is the implicit default
 *  (no badge), non-prod values get a colored chip. */
export function envTone(env: string | undefined | null): 'warn' | 'info' | 'accent' | null {
  if (!env) return null
  const v = env.toLowerCase()
  if (v === 'production' || v === 'prod' || v === 'unknown') return null
  if (v === 'staging' || v === 'stage') return 'warn'
  if (v === 'dev' || v === 'development' || v === 'local') return 'info'
  return 'accent'
}
