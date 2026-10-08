import { effectScope, type EffectScope, nextTick, ref, type Ref } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { NoteThreadFilters } from '~/api/queries'
import type { NoteThreadsPage, NotesSeen, User } from '~/api/types'

/**
 * One visit to the inbox: which marker it measures "new" against, and when it
 * moves the stored one.
 *
 * The two must not be the same value. The visit keeps the marker it started
 * from — so a thread new on arrival stays new, and listed under Unread, while
 * the page is open — and moves the stored one as soon as the list shows, so
 * the badge goes to zero. Getting that backwards either leaves the badge lit
 * or empties Unread the moment it is opened.
 */

const me: Ref<User | undefined> = ref(undefined)
const meRefused = ref(false)
const page: Ref<NoteThreadsPage | undefined> = ref(undefined)
const fetchNotesSeen = vi.fn<() => Promise<NotesSeen>>()
const mutate = vi.fn()
let threadFilters: () => NoteThreadFilters = () => ({})
let threadsEnabled: () => boolean = () => false

vi.mock('@tanstack/vue-query', () => ({
  useQueryClient: () => ({ setQueryData: vi.fn() }),
}))
vi.mock('~/api/queries', () => ({
  NOTES_SEEN_KEY: ['notes', 'seen'],
  fetchNotesSeen: () => fetchNotesSeen(),
  useCurrentUser: () => ({ data: me, isError: meRefused }),
  useMarkNotesSeen: () => ({ mutate }),
  useNoteThreads: (filters: () => NoteThreadFilters, enabled: () => boolean) => {
    threadFilters = filters
    threadsEnabled = enabled
    return { data: page }
  },
}))

const user = { user_id: 'u-anna', username: 'anna', role: 'operator' } as User

function pageWithLatest(latest: string | null): NoteThreadsPage {
  return { threads: [], latest_note_at: latest, has_more: false }
}

/** Let the watchers and the marker request settle. */
async function settle() {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve()
    await nextTick()
  }
}

/** Every visit's scope, stopped after each test — a live one would go on watching the shared refs. */
const scopes: EffectScope[] = []

async function startVisit(show: 'all' | 'mine' | 'unread' = 'all') {
  const { useNotesInbox } = await import('./useNotesInbox')
  const scope = effectScope()
  scopes.push(scope)
  const inbox = scope.run(() => useNotesInbox(() => show, () => 50))!
  await settle()
  return inbox
}

describe('a visit to the notes inbox', () => {
  beforeEach(() => {
    vi.resetModules()
    me.value = undefined
    meRefused.value = false
    page.value = undefined
    fetchNotesSeen.mockReset()
    mutate.mockReset()
  })

  afterEach(() => {
    for (const scope of scopes.splice(0)) scope.stop()
  })

  it('asks for threads only once the marker is known', async () => {
    let answer!: (seen: NotesSeen) => void
    fetchNotesSeen.mockReturnValue(new Promise((resolve) => (answer = resolve)))
    me.value = user

    const inbox = await startVisit('unread')
    expect(inbox.visit.value.state).toBe('deciding')
    expect(threadsEnabled()).toBe(false)

    answer({ seen_at: '2026-10-08T09:00:00.5Z', unread_threads: 2 })
    await settle()

    expect(threadsEnabled()).toBe(true)
    expect(threadFilters()).toEqual({
      mine: false,
      unread: true,
      seen_at: '2026-10-08T09:00:00.5Z',
      limit: 50,
    })
  })

  it('keeps the marker it started from while it moves the stored one', async () => {
    fetchNotesSeen.mockResolvedValue({ seen_at: '2026-10-08T09:00:00Z', unread_threads: 1 })
    me.value = user
    await startVisit()

    page.value = pageWithLatest('2026-10-08T10:00:00Z')
    await settle()

    expect(mutate).toHaveBeenCalledWith('2026-10-08T10:00:00Z')
    // The stored marker moved; what this visit calls new did not.
    expect(threadFilters().seen_at).toBe('2026-10-08T09:00:00Z')
  })

  it('moves the marker once per newest note', async () => {
    fetchNotesSeen.mockResolvedValue({ seen_at: null, unread_threads: 3 })
    me.value = user
    await startVisit()

    page.value = pageWithLatest('2026-10-08T10:00:00Z')
    await settle()
    // A refresh with nothing newer: a new answer, the same newest note.
    page.value = pageWithLatest('2026-10-08T10:00:00Z')
    await settle()
    page.value = pageWithLatest('2026-10-08T10:05:00Z')
    await settle()

    expect(mutate.mock.calls).toEqual([['2026-10-08T10:00:00Z'], ['2026-10-08T10:05:00Z']])
  })

  it('does not move it when nothing is newer than where it stands', async () => {
    fetchNotesSeen.mockResolvedValue({ seen_at: '2026-10-08T10:00:00Z', unread_threads: 0 })
    me.value = user
    await startVisit()

    page.value = pageWithLatest('2026-10-08T10:00:00Z')
    await settle()

    expect(mutate).not.toHaveBeenCalled()
  })

  it('marks nothing and asks nothing about the reader without a user', async () => {
    meRefused.value = true
    const inbox = await startVisit('mine')

    expect(inbox.visit.value.state).toBe('untracked')
    expect(inbox.filter.value).toBe('all')
    expect(threadsEnabled()).toBe(true)
    expect(threadFilters()).toEqual({ limit: 50 })

    page.value = pageWithLatest('2026-10-08T10:00:00Z')
    await settle()
    expect(mutate).not.toHaveBeenCalled()
    expect(fetchNotesSeen).not.toHaveBeenCalled()
  })

  it('still shows the list when the marker cannot be read', async () => {
    fetchNotesSeen.mockRejectedValue(new Error('503'))
    me.value = user
    const inbox = await startVisit()

    expect(inbox.visit.value.state).toBe('untracked')
    expect(threadsEnabled()).toBe(true)
  })
})
