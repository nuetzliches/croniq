import { describe, expect, it } from 'vitest'
import type { NoteThread } from '~/api/types'
import {
  emptyStateFor,
  findThread,
  INBOX_FILTERS,
  markerToPost,
  nextLimit,
  parseInboxFilter,
  selectionFromRoute,
  selectionKey,
  THREADS_MAX,
  THREADS_PAGE,
  threadKey,
  threadKind,
  threadParams,
  threadPath,
} from './notes-inbox'

function thread(partial: Partial<NoteThread>): NoteThread {
  return {
    job_key: 'mail:send',
    execution_id: null,
    execution: null,
    last_note_at: '2026-10-08T09:00:00Z',
    note_count: 1,
    mine: false,
    unread: false,
    notes: [],
    ...partial,
  }
}

describe('the inbox filter', () => {
  it('reads mine and unread from the URL', () => {
    expect(parseInboxFilter('mine')).toBe('mine')
    expect(parseInboxFilter('unread')).toBe('unread')
  })

  it('reads anything else as all', () => {
    for (const raw of [undefined, null, '', 'all', 'Mine', 'yes', ['mine']]) {
      expect(parseInboxFilter(raw)).toBe('all')
    }
  })

  it('offers the three in the order they were asked for', () => {
    expect(INBOX_FILTERS.map((entry) => entry.label)).toEqual(['Mine', 'Unread', 'All'])
  })
})

describe('what a visit asks for', () => {
  const tracking = { state: 'tracking', seenAt: '2026-10-08T09:00:00.123456789Z' } as const

  it('sends the marker the visit started from, verbatim', () => {
    // Not through a `Date`: that would cut it to milliseconds, and the note
    // that set the marker would read as unread again.
    expect(threadParams('all', tracking, 50)).toEqual({
      mine: false,
      unread: false,
      seen_at: '2026-10-08T09:00:00.123456789Z',
      limit: 50,
    })
  })

  it('asks for one narrowing at a time', () => {
    expect(threadParams('mine', tracking, 50)).toMatchObject({ mine: true, unread: false })
    expect(threadParams('unread', tracking, 50)).toMatchObject({ mine: false, unread: true })
  })

  it('sends no marker on a first visit, so everything by others is new', () => {
    expect(threadParams('unread', { state: 'tracking', seenAt: null }, 50).seen_at).toBeNull()
  })

  it('asks nothing about the reader without a marker to measure against', () => {
    expect(threadParams('unread', { state: 'untracked' }, 100)).toEqual({ limit: 100 })
    expect(threadParams('mine', { state: 'deciding' }, 50)).toEqual({ limit: 50 })
  })
})

describe('what gets marked as read', () => {
  it('hands over the newest note when it differs from the last one handed over', () => {
    expect(markerToPost('2026-10-08T10:00:00Z', '2026-10-08T09:00:00Z')).toBe(
      '2026-10-08T10:00:00Z',
    )
  })

  it('says nothing while there are no notes', () => {
    expect(markerToPost(null, null)).toBeNull()
    expect(markerToPost(undefined, '2026-10-08T09:00:00Z')).toBeNull()
  })

  it('does not hand the same value over twice', () => {
    expect(markerToPost('2026-10-08T10:00:00Z', '2026-10-08T10:00:00Z')).toBeNull()
  })

  it('never orders timestamps itself', () => {
    // As strings the earlier instant sorts *after* the later one here: the
    // server writes 6 or 9 fraction digits as it pleases. Deciding "is this
    // newer" in the browser would get it wrong, so the browser does not — it
    // hands the value over, and the server keeps whichever is later.
    const earlier = '2026-10-08T09:00:05.123456Z'
    const later = '2026-10-08T09:00:05.123456700Z'
    expect(earlier > later).toBe(true)
    expect(markerToPost(later, earlier)).toBe(later)
    expect(markerToPost(earlier, later)).toBe(earlier)
  })
})

describe('addressing a thread', () => {
  it('addresses a run thread by its run and a job thread by its job', () => {
    expect(threadPath(thread({ execution_id: 'run-1' }))).toBe('/notes/runs/run-1')
    expect(threadPath(thread({ job_key: 'mail:send' }))).toBe('/notes/jobs/mail%3Asend')
  })

  it('encodes a job key that holds a slash', () => {
    expect(threadPath(thread({ job_key: 'team/backup' }))).toBe('/notes/jobs/team%2Fbackup')
  })

  it('tells a job’s own thread from a thread of one of its runs', () => {
    const own = thread({ job_key: 'mail:send' })
    const run = thread({ job_key: 'mail:send', execution_id: 'run-1' })
    expect(threadKey(own)).not.toBe(threadKey(run))
  })

  it('reads the selection from the route', () => {
    expect(selectionFromRoute('notes-run', { id: 'run-1' })).toEqual({ kind: 'run', id: 'run-1' })
    expect(selectionFromRoute('notes-job', { jobKey: 'mail:send' })).toEqual({
      kind: 'job',
      jobKey: 'mail:send',
    })
    expect(selectionFromRoute('notes', {})).toBeNull()
  })

  it('finds the selected thread, or says it is not there', () => {
    const threads = [
      thread({ job_key: 'mail:send' }),
      thread({ job_key: 'mail:send', execution_id: 'run-1' }),
    ]
    expect(findThread(threads, { kind: 'run', id: 'run-1' })).toBe(threads[1])
    expect(findThread(threads, { kind: 'job', jobKey: 'mail:send' })).toBe(threads[0])
    expect(findThread(threads, { kind: 'run', id: 'run-2' })).toBeNull()
    expect(findThread(threads, null)).toBeNull()
  })

  it('keys a selection the way it keys the thread it names', () => {
    const run = thread({ execution_id: 'run-1' })
    expect(selectionKey({ kind: 'run', id: 'run-1' })).toBe(threadKey(run))
  })
})

describe('what a thread is on', () => {
  it('tells a live run, a deleted run and the job apart', () => {
    expect(threadKind(thread({ execution_id: 'run-1', execution: {} as never }))).toBe('run')
    expect(threadKind(thread({ execution_id: 'run-1', execution: null }))).toBe('run-gone')
    expect(threadKind(thread({ execution_id: null }))).toBe('job')
  })
})

describe('an empty inbox', () => {
  it('says something different for each filter', () => {
    const titles = new Set(['all', 'mine', 'unread'].map((f) => emptyStateFor(parseInboxFilter(f)).title))
    expect(titles.size).toBe(3)
    expect(emptyStateFor('unread').title).toBe('All caught up')
  })
})

describe('showing more', () => {
  it('grows by a page, up to what the server will send', () => {
    expect(nextLimit(THREADS_PAGE)).toBe(2 * THREADS_PAGE)
    expect(nextLimit(THREADS_MAX - 10)).toBe(THREADS_MAX)
    expect(nextLimit(THREADS_MAX)).toBe(THREADS_MAX)
  })
})
