import { describe, expect, it } from 'vitest'
import { parseJobKeySearch } from './job-key-search'

describe('parseJobKeySearch', () => {
  it('searches by substring when unquoted', () => {
    expect(parseJobKeySearch('journal')).toEqual({ contains: 'journal' })
    expect(parseJobKeySearch('  search:journal-sync ')).toEqual({ contains: 'search:journal-sync' })
  })

  it('names one job when quoted', () => {
    expect(parseJobKeySearch('"search:journal-sync"')).toEqual({ exact: 'search:journal-sync' })
    expect(parseJobKeySearch(' " search:journal-sync " ')).toEqual({ exact: 'search:journal-sync' })
  })

  it('accepts typographic quotes', () => {
    expect(parseJobKeySearch('“mail:send”')).toEqual({ exact: 'mail:send' })
    expect(parseJobKeySearch('„mail:send“')).toEqual({ exact: 'mail:send' })
  })

  it('keeps searching while the closing quote is still missing', () => {
    expect(parseJobKeySearch('"mail:se')).toEqual({ contains: 'mail:se' })
    expect(parseJobKeySearch('„mail:se')).toEqual({ contains: 'mail:se' })
    expect(parseJobKeySearch('"')).toEqual({})
  })

  it('asks for nothing when empty', () => {
    expect(parseJobKeySearch('')).toEqual({})
    expect(parseJobKeySearch('   ')).toEqual({})
    expect(parseJobKeySearch('""')).toEqual({})
    expect(parseJobKeySearch('" "')).toEqual({})
  })
})
