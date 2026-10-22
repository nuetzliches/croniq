/**
 * A length typed as `30m` / `2h` / `1d`, turned into the instant it ends.
 *
 * The override and acknowledgement endpoints take a deadline, not a length,
 * because the deadline is what they store and clear on. Converting here rather
 * than asking for a timestamp is the difference between "quiet for an hour"
 * and doing date arithmetic in your head at 3am.
 *
 * `null` for anything else, so the caller can say what it expected.
 */
export function deadlineFrom(input: string, now: number = Date.now()): string | null {
  const match = /^(\d+)\s*(m|h|d)$/i.exec(input.trim())
  if (!match) return null
  const value = Number.parseInt(match[1]!, 10)
  const ms = { m: 60_000, h: 3_600_000, d: 86_400_000 }[match[2]!.toLowerCase() as 'm' | 'h' | 'd']
  return new Date(now + value * ms).toISOString()
}
