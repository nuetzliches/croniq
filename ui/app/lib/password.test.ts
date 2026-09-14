import { describe, expect, it } from 'vitest'
import { checkPassword, PASSWORD_MAX_BYTES, PASSWORD_MIN_LEN } from './password'

/**
 * These mirror `croniq_auth::password::validate_password`. If the server ever
 * moves a bound, these fail — which is the point: a form that promises looser
 * rules than the server enforces sends people into a 400 with no explanation.
 */
describe('checkPassword', () => {
  it('refuses anything shorter than the server will accept', () => {
    expect(checkPassword('a'.repeat(PASSWORD_MIN_LEN - 1))).toMatch(/at least 8/)
    expect(checkPassword('a'.repeat(PASSWORD_MIN_LEN))).toBeNull()
  })

  it('measures the upper bound in bytes, because bcrypt does', () => {
    expect(checkPassword('a'.repeat(PASSWORD_MAX_BYTES))).toBeNull()
    expect(checkPassword('a'.repeat(PASSWORD_MAX_BYTES + 1))).toMatch(/72 bytes/)
  })

  /**
   * The case a character count would get wrong. Eighteen four-byte emoji are
   * 72 bytes and fit; nineteen are 76 and do not — while both are far under
   * any plausible character limit.
   */
  it('counts a four-byte character as four', () => {
    expect(checkPassword('😀'.repeat(18))).toBeNull()
    expect(checkPassword('😀'.repeat(19))).toMatch(/72 bytes/)
  })
})
