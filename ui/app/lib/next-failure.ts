/**
 * What the Runs screen's "Next failure" button jumps to.
 *
 * `dead` counts as well as `failed`: a dead run is one whose retries ran out,
 * which is a failure with nothing left to try, and its pill is just as red.
 */
export const FAILURE_STATES: ReadonlySet<string> = new Set(['failed', 'dead'])

/**
 * The index of the first failure after `from`, wrapping round to the top, or
 * -1 when there is none.
 *
 * `from` is the list's cursor, so -1 (nothing selected yet) finds the first
 * failure, and repeated jumps walk down the list and start over at the end. A
 * single failure already under the cursor answers with itself, so the button
 * keeps scrolling back to it rather than doing nothing.
 */
export function nextFailureIndex(states: readonly string[], from: number): number {
  const count = states.length
  for (let step = 1; step <= count; step += 1) {
    const index = (((from + step) % count) + count) % count
    if (FAILURE_STATES.has(states[index]!)) return index
  }
  return -1
}
