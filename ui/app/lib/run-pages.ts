/**
 * How the runs list joins its live page with the older pages behind it.
 *
 * Generic over anything with an `id`, so the tests need no full `Execution`.
 */

/**
 * How many older pages "Load older" may add on top of the live one.
 *
 * The list is not virtualised, and every page stayed in the DOM and in the
 * de-duplication that runs on each five-second poll: a long session of paging
 * back grew both without bound (review 2026-10-24, U-4). Ten pages of 200 plus
 * the live page is 2 200 rows — as many as the console keeps, and well past
 * what anyone scrolls through by hand. Beyond that the filters and the time
 * window are the faster way back, and the list says so.
 */
export const MAX_OLDER_PAGES = 10

/**
 * The live page first, then the older pages, each run once.
 *
 * The live page comes **first** so its copy of a row wins: both halves can hold
 * the same run (the overlap is deliberate), and the live one is the one that
 * has been refetched.
 */
export function mergeRunPages<T extends { id: string }>(
  live: readonly T[],
  older: readonly (readonly T[])[],
): T[] {
  const seen = new Set<string>()
  const out: T[] = []
  for (const list of [live, ...older]) {
    for (const row of list) {
      if (seen.has(row.id)) continue
      seen.add(row.id)
      out.push(row)
    }
  }
  return out
}

/**
 * What the foot of the list offers.
 *
 * `more`: a full page came back last, so there is probably more behind it.
 * `limit`: there may be more, but the list holds all it will hold.
 * `end`: the last page came back short; that is everything.
 */
export function pagingState(
  livePageLength: number,
  older: readonly (readonly unknown[])[],
  pageSize: number,
): 'more' | 'limit' | 'end' {
  const lastLength = older.at(-1)?.length ?? livePageLength
  if (lastLength < pageSize) return 'end'
  return older.length >= MAX_OLDER_PAGES ? 'limit' : 'more'
}
