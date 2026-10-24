import { describe, expect, it } from 'vitest'
import { MAX_OLDER_PAGES, mergeRunPages, pagingState } from './run-pages'

const page = (prefix: string, n: number, version = 'v1') =>
  Array.from({ length: n }, (_, i) => ({ id: `${prefix}${i}`, version }))

describe('mergeRunPages', () => {
  it('keeps the live copy of a run that is on both pages', () => {
    const live = [{ id: 'a', version: 'live' }, { id: 'b', version: 'live' }]
    const older = [[{ id: 'b', version: 'old' }, { id: 'c', version: 'old' }]]

    expect(mergeRunPages(live, older)).toEqual([
      { id: 'a', version: 'live' },
      { id: 'b', version: 'live' },
      { id: 'c', version: 'old' },
    ])
  })

  it('keeps page order: live first, then oldest page last', () => {
    const merged = mergeRunPages(page('l', 2), [page('o1-', 2), page('o2-', 1)])
    expect(merged.map((row) => row.id)).toEqual(['l0', 'l1', 'o1-0', 'o1-1', 'o2-0'])
  })
})

describe('pagingState', () => {
  it('offers more after a full page', () => {
    expect(pagingState(200, [], 200)).toBe('more')
    expect(pagingState(200, [page('o', 200)], 200)).toBe('more')
  })

  it('reports the end after a short page', () => {
    expect(pagingState(37, [], 200)).toBe('end')
    expect(pagingState(200, [page('o', 200), page('p', 12)], 200)).toBe('end')
  })

  it('stops offering pages at the cap', () => {
    // Review 2026-10-24, U-4: "Load older" accumulated pages without bound in
    // a list that is not virtualised and is re-merged on every poll.
    const full = Array.from({ length: MAX_OLDER_PAGES }, (_, i) => page(`p${i}-`, 200))
    expect(pagingState(200, full.slice(0, -1), 200)).toBe('more')
    expect(pagingState(200, full, 200)).toBe('limit')
  })

  it('prefers the end to the cap when the last page was short', () => {
    const pages = [
      ...Array.from({ length: MAX_OLDER_PAGES - 1 }, (_, i) => page(`p${i}-`, 200)),
      page('last', 3),
    ]
    expect(pagingState(200, pages, 200)).toBe('end')
  })
})
