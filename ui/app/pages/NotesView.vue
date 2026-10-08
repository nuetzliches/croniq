<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { storeToRefs } from 'pinia'
import type { JobNote, NoteThread } from '~/api/types'
import NoteBadge from '~/components/NoteBadge.vue'
import NoteThreadDetail from '~/components/NoteThreadDetail.vue'
import RunDetail from '~/components/RunDetail.vue'
import { useDetailPaneWidth } from '~/composables/useDetailPaneWidth'
import { useNotesInbox } from '~/composables/useNotesInbox'
import { useRevealSelected } from '~/composables/useRevealSelected'
import { useTimeDisplay } from '~/composables/useTimeDisplay'
import { shortId } from '~/lib/format'
import { noteExcerpt, noteIcon, noteLabel, noteTone } from '~/lib/notes'
import {
  emptyStateFor,
  findThread,
  INBOX_FILTERS,
  type InboxFilter,
  nextLimit,
  parseInboxFilter,
  selectionFromRoute,
  selectionKey,
  THREADS_MAX,
  THREADS_PAGE,
  threadKey,
  threadKind,
  threadPath,
} from '~/lib/notes-inbox'
import { DETAIL_PANE_DEFAULT_WIDTH, DETAIL_PANE_MIN_WIDTH, useUiStore } from '~/stores/ui'

/**
 * Notes — every thread with a note in it, across all jobs.
 *
 * Its own screen rather than a filter on Runs, for the reason Dead Letters is
 * one (docs/ui-screen-inventory.md): it is a work list — what colleagues have
 * said since you last looked — and a row is a thread, not a run. A job's own
 * notes have no run to be a row of, and a run's notes outlive the run.
 *
 * Opening the screen is what marks everything read (`useNotesInbox`). What was
 * new on arrival keeps its dot for the rest of the visit, and Unread keeps
 * listing it; the badge in the navigation is zero the moment the list shows.
 */
const route = useRoute()
const router = useRouter()

/** Mine · Unread · All, in the URL like every list filter here; absent is All. */
const show = computed<InboxFilter>(() => parseInboxFilter(route.query.show))

/** Grows with "Show more"; a filter change starts again from one page. */
const limit = ref(THREADS_PAGE)
watch(show, () => {
  limit.value = THREADS_PAGE
})

const { visit, filter, threads: query } = useNotesInbox(show, limit)
const { data, isPending, isError, error, refetch } = query

/** Whether this visit knows what is new: a user's session whose marker was read. */
const tracking = computed(() => visit.value.state === 'tracking')
const loading = computed(() => visit.value.state === 'deciding' || (isPending.value && !data.value))

interface Row {
  key: string
  thread: NoteThread
  latest: JobNote | undefined
  kind: ReturnType<typeof threadKind>
  isNew: boolean
}

/** In the server's order: by newest note, which is the order that matters. */
const rows = computed<Row[]>(() =>
  (data.value?.threads ?? []).map((thread) => ({
    key: threadKey(thread),
    thread,
    latest: thread.notes[0],
    kind: threadKind(thread),
    isNew: tracking.value && thread.unread,
  })),
)
const newCount = computed(() => rows.value.filter((row) => row.isNew).length)
const hasMore = computed(() => Boolean(data.value?.has_more))
const empty = computed(() => emptyStateFor(filter.value))

const selection = computed(() => selectionFromRoute(route.name, route.params))
const selectedKey = computed(() => (selection.value ? selectionKey(selection.value) : undefined))
const selected = computed(() =>
  findThread(data.value?.threads ?? [], selection.value),
)

/** Shares its width with Runs: most threads open the same run detail. */
const { runDetailWidth } = storeToRefs(useUiStore())
const splitEl = ref<HTMLElement | null>(null)
const { max: maxDetailWidth, width: detailWidth } = useDetailPaneWidth(splitEl, runDetailWidth)

function setShow(value: InboxFilter) {
  const next = { ...route.query }
  if (value === 'all') delete next.show
  else next.show = value
  // `replace`: a filter is not a place to come back to with the back button.
  // And `/notes`, so the thread open under the old filter closes with it.
  void router.replace({ path: '/notes', query: next })
}

function open(thread: NoteThread) {
  void router.push({ path: threadPath(thread), query: route.query })
}

function close() {
  void router.push({ path: '/notes', query: route.query })
}

function showMore() {
  limit.value = nextLimit(limit.value)
}

/** "Last note" as "3 min ago" or as the clock time — see `useTimeDisplay`. */
const lastNote = useTimeDisplay()

/** Keyboard: j/k to move, Enter to open, Escape to close — as on Runs. */
const cursor = ref(-1)
watch(rows, (next) => {
  if (cursor.value >= next.length) cursor.value = next.length - 1
})

const listEl = ref<HTMLElement | null>(null)
/** A link to a thread scrolls the list to it. */
useRevealSelected(listEl, selectedKey, () => rows.value.length)

function onKey(event: KeyboardEvent) {
  const target = event.target as HTMLElement | null
  // Never steal keys from a field someone is typing in — the composer in
  // the detail is one.
  if (target && ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName)) return
  if (event.key === 'Enter' && target && ['BUTTON', 'A'].includes(target.tagName)) return

  if (event.key === 'j' || event.key === 'ArrowDown') {
    cursor.value = Math.min(cursor.value + 1, rows.value.length - 1)
    event.preventDefault()
  } else if (event.key === 'k' || event.key === 'ArrowUp') {
    cursor.value = Math.max(cursor.value - 1, 0)
    event.preventDefault()
  } else if (event.key === 'Enter' && cursor.value >= 0) {
    const row = rows.value[cursor.value]
    if (row) open(row.thread)
  } else if (event.key === 'Escape' && selection.value) {
    close()
  }
}
</script>

<template>
  <div
    class="flex h-full min-h-0 flex-col gap-4"
    tabindex="-1"
    @keydown="onKey"
  >
    <div class="flex flex-wrap items-center gap-2">
      <!--
        One of three, so a radio group — the pattern the note composer uses.
        Hidden without a marker to measure against: an API-key session has no
        "mine" or "unread" to ask about.
      -->
      <div
        v-if="tracking"
        role="radiogroup"
        aria-label="Show threads"
        class="inline-flex items-center gap-0.5 rounded-lg border border-default p-0.5"
      >
        <UButton
          v-for="option in INBOX_FILTERS"
          :key="option.value"
          role="radio"
          :aria-checked="show === option.value"
          size="sm"
          :color="show === option.value ? 'primary' : 'neutral'"
          :variant="show === option.value ? 'soft' : 'ghost'"
          :data-testid="`notes-show-${option.value}`"
          @click="setShow(option.value)"
        >
          {{ option.label }}
        </UButton>
      </div>

      <span class="cq-num ml-auto text-sm text-muted">
        {{ rows.length }} {{ rows.length === 1 ? 'thread' : 'threads' }}<template v-if="newCount"> · {{ newCount }} new</template>
      </span>
    </div>

    <div
      ref="splitEl"
      class="flex min-h-0 flex-1 gap-1.5"
    >
      <div
        ref="listEl"
        class="cq-list min-w-0 flex-1"
      >
        <AppLoading
          v-if="loading"
          label="Loading notes"
        />
        <AppError
          v-else-if="isError"
          :error="error"
          :on-retry="() => refetch()"
        />
        <AppEmpty
          v-else-if="rows.length === 0"
          :icon="empty.icon"
          :title="empty.title"
          :description="empty.description"
        >
          <template
            v-if="filter !== 'all'"
            #action
          >
            <UButton
              variant="subtle"
              color="neutral"
              @click="setShow('all')"
            >
              Show all threads
            </UButton>
          </template>
        </AppEmpty>

        <table
          v-else
          class="w-full border-collapse"
        >
          <thead class="sticky top-0 z-10 bg-default">
            <tr class="border-b border-default">
              <th class="w-0 py-[var(--cq-cell-y)] pl-[var(--cq-cell-x)]">
                <span class="sr-only">Unread</span>
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
                State
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
                Job
              </th>
              <th
                v-if="!selection"
                class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left"
              >
                Run
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
                Latest note
              </th>
              <th
                v-if="!selection"
                class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left"
              >
                Notes
              </th>
              <th
                class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-right whitespace-nowrap"
              >
                <TimeHeading label="Last note" />
              </th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="(row, index) in rows"
              :key="row.key"
              :data-row-index="index"
              :data-unread="row.isNew || undefined"
              :data-selected="row.key === selectedKey || undefined"
              :class="[
                'cq-row',
                'cursor-pointer border-b border-default/60 transition-colors hover:bg-elevated',
                row.key === selectedKey && 'bg-elevated',
                index === cursor && row.key !== selectedKey && 'ring-1 ring-primary/40 ring-inset',
              ]"
              @click="open(row.thread)"
            >
              <td class="py-[var(--cq-cell-y)] pl-[var(--cq-cell-x)]">
                <template v-if="row.isNew">
                  <span
                    class="block size-2 rounded-full bg-primary"
                    aria-hidden="true"
                  />
                  <span class="sr-only">Unread</span>
                </template>
              </td>
              <td class="px-[var(--cq-cell-x)]">
                <StatusPill
                  v-if="row.thread.execution"
                  :state="row.thread.execution.state"
                />
                <UBadge
                  v-else-if="row.kind === 'run-gone'"
                  v-tooltip="'Retention has deleted this run; its notes stay'"
                  color="neutral"
                  variant="outline"
                  size="sm"
                  icon="i-lucide-archive"
                >
                  run deleted
                </UBadge>
                <UBadge
                  v-else
                  v-tooltip="'Notes on the job itself, not on one of its runs'"
                  color="neutral"
                  variant="outline"
                  size="sm"
                  icon="i-lucide-briefcase"
                >
                  job
                </UBadge>
              </td>
              <td
                v-tooltip="row.thread.job_key"
                class="max-w-[16rem] truncate px-[var(--cq-cell-x)] font-mono text-primary"
              >
                {{ row.thread.job_key }}
              </td>
              <td
                v-if="!selection"
                v-tooltip="row.thread.execution_id ?? undefined"
                class="cq-num px-[var(--cq-cell-x)] font-mono text-muted"
              >
                {{ shortId(row.thread.execution_id) }}
              </td>
              <td class="w-full max-w-0 px-[var(--cq-cell-x)]">
                <div
                  v-if="row.latest"
                  class="flex min-w-0 items-center gap-2 overflow-hidden text-sm"
                >
                  <UIcon
                    :name="noteIcon(row.latest.kind)"
                    class="size-4 shrink-0"
                    :class="noteTone(row.latest.kind)"
                    aria-hidden="true"
                  />
                  <span class="sr-only">{{ noteLabel(row.latest.kind) }}:</span>
                  <!-- Capped, so a long name — an API key's is its client
                       id — leaves the note itself some room. -->
                  <span
                    class="max-w-[45%] shrink-0 truncate"
                    :class="row.isNew ? 'font-semibold text-highlighted' : 'font-medium'"
                  >{{ row.latest.author_name }}</span>
                  <span
                    class="min-w-0 flex-1 truncate"
                    :class="row.isNew ? 'text-highlighted' : 'text-muted'"
                  >{{ noteExcerpt(row.latest) }}</span>
                </div>
              </td>
              <td
                v-if="!selection"
                class="px-[var(--cq-cell-x)]"
              >
                <div class="flex items-center gap-1.5 whitespace-nowrap">
                  <NoteBadge :notes="row.thread.notes" />
                  <!-- The badge counts what came along: the newest few. -->
                  <span
                    v-if="row.thread.note_count > row.thread.notes.length"
                    class="cq-num text-xs text-muted"
                  >{{ row.thread.note_count }} notes</span>
                </div>
              </td>
              <td
                v-tooltip="lastNote.title(row.thread.last_note_at)"
                class="cq-num px-[var(--cq-cell-x)] text-right whitespace-nowrap text-muted"
              >
                {{ lastNote.text(row.thread.last_note_at) }}
              </td>
            </tr>
          </tbody>
        </table>

        <!--
          More by asking again for a longer page, not by cursor: a reply moves
          its thread to the top, and an appended page would show it twice.
        -->
        <div
          v-if="rows.length > 0"
          class="flex items-center justify-center gap-3 border-t border-default px-3 py-3"
        >
          <UButton
            v-if="hasMore && limit < THREADS_MAX"
            color="neutral"
            variant="subtle"
            size="xs"
            icon="i-lucide-arrow-down"
            @click="showMore"
          >
            Show more
          </UButton>
          <span
            v-else-if="hasMore"
            class="text-xs text-muted"
          >The newest {{ THREADS_MAX }} threads. Narrow the list to see further back.</span>
          <span
            v-else
            class="text-xs text-muted"
          >That is everything.</span>
        </div>
      </div>

      <template v-if="selection">
        <PaneResizer
          v-model="runDetailWidth"
          :min="DETAIL_PANE_MIN_WIDTH"
          :max="maxDetailWidth"
          :default-width="DETAIL_PANE_DEFAULT_WIDTH"
          label="Resize thread detail"
        />
        <!-- A run that still exists opens as on Runs, notes and log together. -->
        <RunDetail
          v-if="selected?.execution"
          :execution="selected.execution"
          class="shrink-0"
          :style="{ width: `${detailWidth}px` }"
          @close="close"
        />
        <NoteThreadDetail
          v-else
          :thread="selected"
          :selection="selection"
          :filtered="filter !== 'all'"
          class="shrink-0"
          :style="{ width: `${detailWidth}px` }"
          @close="close"
          @show-all="setShow('all')"
        />
      </template>
    </div>
  </div>
</template>
