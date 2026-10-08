import type { NoteThreadFilters } from '~/api/queries'
import type { NoteThread } from '~/api/types'

/**
 * Framework-free logic of the notes inbox, kept apart from the page so what
 * it asks for and what it marks can be tested without a component harness
 * (#677).
 */

/** The three ways to look at the inbox. */
export type InboxFilter = 'all' | 'mine' | 'unread'

/** In the order the toolbar offers them. */
export const INBOX_FILTERS: ReadonlyArray<{ value: InboxFilter; label: string }> = [
  { value: 'mine', label: 'Mine' },
  { value: 'unread', label: 'Unread' },
  { value: 'all', label: 'All' },
]

/** `?show=` as a filter. Anything else, or nothing, is everything. */
export function parseInboxFilter(raw: unknown): InboxFilter {
  return raw === 'mine' || raw === 'unread' ? raw : 'all'
}

/**
 * Where one visit to the inbox stands.
 *
 * `deciding` until it is known whether there is a user to keep a marker for.
 * `untracked` when there is none — an API-key session — or when the marker
 * could not be read: the list still shows, nothing is marked or highlighted.
 * `tracking` with the marker the visit started from, held for the whole visit,
 * so what was new on arrival stays marked as new until the next one.
 */
export type NotesVisit =
  | { state: 'deciding' }
  | { state: 'untracked' }
  | { state: 'tracking'; seenAt: string | null }

/**
 * What to ask the server for.
 *
 * Every request of a visit carries the marker it started from, never the one
 * the visit has since moved: that is what keeps a thread that was new on
 * arrival listed under Unread after the badge has gone to zero. Without a
 * marker there is no "mine" or "unread" to ask about — only the list.
 */
export function threadParams(
  filter: InboxFilter,
  visit: NotesVisit,
  limit: number,
): NoteThreadFilters {
  if (visit.state !== 'tracking') return { limit }
  return {
    mine: filter === 'mine',
    unread: filter === 'unread',
    seen_at: visit.seenAt,
    limit,
  }
}

/**
 * The marker to hand the server, if any: the newest note a page carried, when
 * it differs from the last one handed over.
 *
 * Inequality, never order. The server writes 3, 6 or 9 fraction digits, so as
 * strings `…05.123456Z` sorts after `…05.123456700Z` though it is the earlier
 * instant. The server keeps whichever is later, so the dashboard need not
 * know which one that is.
 */
export function markerToPost(
  latest: string | null | undefined,
  lastPosted: string | null,
): string | null {
  if (!latest || latest === lastPosted) return null
  return latest
}

/** The thread a route names: a run's, or a job's own. */
export type ThreadSelection = { kind: 'run'; id: string } | { kind: 'job'; jobKey: string }

export function selectionFromRoute(
  name: unknown,
  params: Record<string, unknown>,
): ThreadSelection | null {
  if (name === 'notes-run' && typeof params.id === 'string') return { kind: 'run', id: params.id }
  if (name === 'notes-job' && typeof params.jobKey === 'string') {
    return { kind: 'job', jobKey: params.jobKey }
  }
  return null
}

type ThreadRef = Pick<NoteThread, 'job_key' | 'execution_id'>

/** A thread's identity: its run, or for a job's own notes, its job. */
export function threadKey(thread: ThreadRef): string {
  return thread.execution_id ? `run:${thread.execution_id}` : `job:${thread.job_key}`
}

export function selectionKey(selection: ThreadSelection): string {
  return selection.kind === 'run' ? `run:${selection.id}` : `job:${selection.jobKey}`
}

/** Where a thread opens. A job key may hold `:` or `/`, so it is encoded. */
export function threadPath(thread: ThreadRef): string {
  return thread.execution_id
    ? `/notes/runs/${thread.execution_id}`
    : `/notes/jobs/${encodeURIComponent(thread.job_key)}`
}

export function findThread(
  threads: readonly NoteThread[],
  selection: ThreadSelection | null,
): NoteThread | null {
  if (!selection) return null
  const key = selectionKey(selection)
  return threads.find((thread) => threadKey(thread) === key) ?? null
}

/** What a thread's notes are on: a run, a run retention has deleted, or the job. */
export function threadKind(
  thread: Pick<NoteThread, 'execution_id' | 'execution'>,
): 'run' | 'run-gone' | 'job' {
  if (!thread.execution_id) return 'job'
  return thread.execution ? 'run' : 'run-gone'
}

/** What an empty inbox says, per filter. Icon names are literals (ADR-0005). */
export function emptyStateFor(filter: InboxFilter): {
  icon: string
  title: string
  description: string
} {
  switch (filter) {
    case 'mine':
      return {
        icon: 'i-lucide-message-square-dashed',
        title: 'Nothing of yours yet',
        description:
          'Threads you have written in show up here, with what colleagues answered.',
      }
    case 'unread':
      return {
        icon: 'i-lucide-check-check',
        title: 'All caught up',
        description: 'Nothing new from colleagues since you last looked.',
      }
    default:
      return {
        icon: 'i-lucide-messages-square',
        title: 'No notes yet',
        description:
          'Mark a failed run as checked, or leave a question on it, and it shows up here for everyone.',
      }
  }
}

/** One page of threads, and the most "Show more" grows to — the server's cap. */
export const THREADS_PAGE = 50
export const THREADS_MAX = 200

export function nextLimit(current: number): number {
  return Math.min(current + THREADS_PAGE, THREADS_MAX)
}
