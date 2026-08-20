import { describe, expect, it } from 'vitest'
import { declaringKeyVar } from './env-managed'

describe('declaringKeyVar', () => {
  it('names CRONIQ_API_KEY for the default client', () => {
    // Issue #481: the previous inline template produced
    // CRONIQ_API_CLIENT_DEFAULT_KEY, which is a *second* declaration of the
    // same client — the server rejects that at boot, so acting on the hint
    // broke the next start.
    expect(declaringKeyVar('default')).toBe('CRONIQ_API_KEY')
    expect(declaringKeyVar('default')).not.toContain('CRONIQ_API_CLIENT_')
  })

  it('maps a named client to its prefixed variable', () => {
    expect(declaringKeyVar('reporting')).toBe('CRONIQ_API_CLIENT_REPORTING_KEY')
  })

  it('maps dashes to underscores, matching the server grammar', () => {
    // `CRONIQ_API_CLIENT_RUNNER_POLL_KEY` declares the client `runner-poll`,
    // so the reverse has to restore the underscore.
    expect(declaringKeyVar('runner-poll')).toBe('CRONIQ_API_CLIENT_RUNNER_POLL_KEY')
    expect(declaringKeyVar('a-b-c')).toBe('CRONIQ_API_CLIENT_A_B_C_KEY')
  })
})
