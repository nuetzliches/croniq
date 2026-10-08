import { useQueryClient } from '@tanstack/vue-query'
import { computed, type MaybeRefOrGetter, ref, toValue, watch } from 'vue'
import {
  fetchNotesSeen,
  NOTES_SEEN_KEY,
  useCurrentUser,
  useMarkNotesSeen,
  useNoteThreads,
} from '~/api/queries'
import { type InboxFilter, markerToPost, type NotesVisit, threadParams } from '~/lib/notes-inbox'

/**
 * One visit to the notes inbox: the list, and the marker it moves.
 *
 * On arrival the visit reads the stored marker once and keeps it. Every
 * request carries that value, so what was new on arrival stays new — and
 * listed under Unread — for as long as the page is open. Each answer that
 * brings a newer note hands that note back to the server, which moves the
 * marker and answers with the badge: zero, while the reader is looking. The
 * next visit starts from there, and those threads are no longer new.
 *
 * A visit is the page's component instance. Opening a thread is a route change
 * within the same instance, so it keeps the visit; leaving the inbox, or
 * reloading, starts the next one.
 */
export function useNotesInbox(
  show: MaybeRefOrGetter<InboxFilter>,
  limit: MaybeRefOrGetter<number>,
) {
  const queryClient = useQueryClient()
  const { data: me, isError: noUser } = useCurrentUser()
  const visit = ref<NotesVisit>({ state: 'deciding' })

  let decided = false
  watch(
    [me, noUser],
    async ([user, refused]) => {
      if (decided) return
      // An API-key session has no user, and `/v1/users/me` refuses it.
      if (refused) {
        decided = true
        visit.value = { state: 'untracked' }
        return
      }
      if (!user) return
      decided = true
      try {
        // Asked for directly rather than taken from the badge's cache, which
        // may be half a minute old: this one value decides what reads as new
        // for the whole visit.
        const seen = await fetchNotesSeen()
        queryClient.setQueryData(NOTES_SEEN_KEY, seen)
        visit.value = { state: 'tracking', seenAt: seen.seen_at }
      } catch {
        // The list is still worth showing; it just cannot say what is new.
        visit.value = { state: 'untracked' }
      }
    },
    { immediate: true },
  )

  /** Mine and Unread need a marker to be about; without one, everything. */
  const filter = computed<InboxFilter>(() =>
    visit.value.state === 'tracking' ? toValue(show) : 'all',
  )

  const threads = useNoteThreads(
    () => threadParams(filter.value, visit.value, toValue(limit)),
    () => visit.value.state !== 'deciding',
  )

  const mark = useMarkNotesSeen()
  let lastPosted: string | null = null
  watch(
    [visit, () => threads.data.value?.latest_note_at],
    ([current, latest]) => {
      if (current.state !== 'tracking') return
      const next = markerToPost(latest, lastPosted ?? current.seenAt)
      if (!next) return
      lastPosted = next
      mark.mutate(next)
    },
    { immediate: true },
  )

  return { visit, filter, threads }
}
