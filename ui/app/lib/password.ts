/**
 * The password rules, mirrored from the server.
 *
 * `croniq_auth::password::validate_password` is the authority and refuses with
 * a 400. Repeating the bounds here is not a second implementation of the rule
 * — it is so a form can say "at least eight characters" *before* the round
 * trip, rather than after it, and so the two account-recovery screens phrase
 * the same requirement the same way.
 *
 * The upper bound is the interesting one and the reason it is stated at all:
 * bcrypt silently ignores everything past 72 bytes, so a passphrase longer
 * than that would be accepted, truncated, and then fail to match whatever the
 * person believes they typed. The server rejects it rather than truncate;
 * saying so up front is kinder than a 400 with no explanation.
 */
export const PASSWORD_MIN_LEN = 8
export const PASSWORD_MAX_BYTES = 72

export const PASSWORD_RULE = `At least ${PASSWORD_MIN_LEN} characters, at most ${PASSWORD_MAX_BYTES} bytes.`

/** `null` when acceptable, otherwise why not — in the same words as the rule. */
export function checkPassword(password: string): string | null {
  if (password.length < PASSWORD_MIN_LEN) {
    return `A password needs at least ${PASSWORD_MIN_LEN} characters.`
  }
  // Bytes, not characters: the server measures the UTF-8 length because that
  // is what bcrypt truncates, so an emoji costs four of the budget.
  if (new TextEncoder().encode(password).length > PASSWORD_MAX_BYTES) {
    return `A password may be at most ${PASSWORD_MAX_BYTES} bytes — bcrypt ignores anything longer, so a longer one would be silently cut short.`
  }
  return null
}
