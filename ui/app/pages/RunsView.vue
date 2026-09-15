<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { fetchExecutions, useExecutions } from '~/api/queries'
import type { Execution } from '~/api/types'
import { formatAbsolute, formatDuration, formatRelative, shortId } from '~/lib/format'
import { useDebounced } from '~/composables/useDebounced'

/**
 * Runs — the one list of executions.
 *
 * This screen replaces three in the React tree (the standalone Executions
 * page, the job detail's two executions tabs) plus the runner detail's block,
 * per docs/ui-screen-inventory.md. Everything that used to render its own
 * table now links in here with a filter.
 *
 * Two things the audit said about its predecessor, and what changed:
 *
 * It spent ~88px a row on three-line cards, so seven runs were visible where a
 * table shows twenty-five — and with no columns you could not compare
 * durations down the page at all. This is a table, at the density the tokens
 * in main.css set.
 *
 * And it reserved ~60% of the viewport for a detail pane that was empty until
 * you clicked something. Here the table is full width until a run is selected,
 * and the detail opens beside it — addressable at /executions/:id, so a link
 * still works, without the list losing its place.
 */
const route = useRoute()
const router = useRouter()

/**
 * Filters live in the URL, not in component state. That is the contract the
 * Playwright suite asserts and the reason a filtered view can be pasted into a
 * ticket.
 */
const filters = computed(() => ({
  state: (route.query.state as string) || '',
  job_key: (route.query.job_key as string) || '',
  runner_id: (route.query.runner_id as string) || '',
  window: (route.query.window as string) || '',
}))

/**
 * The time window, as a length rather than two instants.
 *
 * "The last hour" is the question people actually have; two datetime pickers
 * make them do arithmetic to ask it. The URL carries the length, so a pasted
 * link means "the last hour" whenever it is opened rather than freezing a
 * window around when it was copied — which is almost always what the sender
 * meant.
 *
 * The length is what travels — into the URL, and into the query. Resolving it
 * to an instant is `useExecutions`' job, on each fetch, because an instant
 * computed here would be computed once: a `computed` depends on the window
 * filter and not on the clock, so "the last hour" would stay pinned to the
 * moment the tab was opened and grow all day (issue #662).
 */
const WINDOWS = [
  { label: 'Last hour', value: '1h', ms: 3_600_000 },
  { label: 'Last 24 hours', value: '24h', ms: 86_400_000 },
  { label: 'Last 7 days', value: '7d', ms: 604_800_000 },
]

/**
 * The typed filter, trailing the box by a beat.
 *
 * `job_key` is the only one typed a character at a time; `state` and `window`
 * are chosen from a menu and `runner_id` arrives from a link, so those stay
 * immediate (issue #730).
 */
const typedJobKey = useDebounced(() => filters.value.job_key)

const sinceMs = computed(
    () => WINDOWS.find((entry) => entry.value === filters.value.window)?.ms,
)

/** How many rows a page asks for, live or older. */
const PAGE_SIZE = 200

/**
 * Older pages, oldest page last. Reset whenever a filter changes.
 *
 * These are fetched once each and not polled, while the query below keeps
 * polling the newest page. That split is the point: paging used to move the
 * *live* query's `until` back, which turned the whole screen into a snapshot
 * — after one click on "Load older" no new run ever appeared again, and a row
 * sitting at `queued` never advanced (issue #662).
 *
 * Older runs are overwhelmingly finished, so not polling them costs nothing;
 * anything still moving is in the live page, which overlaps these and wins the
 * de-duplication below.
 */
const pages = ref<Execution[][]>([])
/** In flight, so the button can say so and cannot be clicked twice. */
const loadingOlder = ref(false)
/**
 * Bumped whenever the filters change or a page is requested, so a response
 * that belongs to a superseded question can be dropped rather than shown.
 */
let filterGeneration = 0

// A getter, not a value — see useExecutions. Passing `filters.value` here is
// the mistake that makes the list freeze on its first filter.
//
// No `until` here: this is the live head of the list and it must stay that
// way. The cursor lives in `loadOlder`, which uses it once and throws it away.
const { data, isPending, isError, error, refetch } = useExecutions(() => ({
  state: filters.value.state || undefined,
  job_key: typedJobKey.value || undefined,
  runner_id: filters.value.runner_id || undefined,
  since_ms: sinceMs.value,
  limit: PAGE_SIZE,
}))

/**
 * Everything on screen: the live page, then the older ones beneath it.
 *
 * The live page comes **first** so its copy of a row wins the de-duplication.
 * Both halves can hold the same run — the overlap is deliberate — and the live
 * one is the one that has been refetched.
 */
const rows = computed<Execution[]>(() => {
  const seen = new Set<string>()
  const out: Execution[] = []
  for (const row of [...(data.value ?? []), ...pages.value.flat()]) {
    if (seen.has(row.id)) continue
    seen.add(row.id)
    out.push(row)
  }
  return out
})

/** A full page back means there is probably more behind it. */
const mayHaveMore = computed(() => {
  const last = pages.value.at(-1) ?? data.value ?? []
  return last.length >= PAGE_SIZE
})

/**
 * Fetch one page older than everything on screen, once.
 *
 * Deliberately not a `useQuery`: an older page is a snapshot of finished work,
 * and giving each one a polling query would multiply the request rate by the
 * number of times someone clicked. The cursor is used here and discarded — it
 * never reaches the live query, which is what #662 was about.
 *
 * The `created_at` passed back is the server's own value, verbatim.
 * Reconstructing one from a `Date` truncates to milliseconds, and the server
 * compares at full precision: a truncated cursor sorts below a row inside that
 * instant and drops it. The `id` alongside it is what gets the cursor past a
 * page-sized group of runs sharing one timestamp (issue #654).
 */
async function loadOlder() {
  const oldest = rows.value.at(-1)
  if (!oldest || loadingOlder.value) return
  loadingOlder.value = true
  // Which filter set this page was asked for. The watcher below clears `pages`
  // on a filter change, but it runs separately from this request — so a page
  // already in flight used to be appended underneath rows fetched with the new
  // filter, and the list showed two different questions' answers as one
  // (issue #721).
  const asked = ++filterGeneration
  try {
    const page = await fetchExecutions({
      state: filters.value.state || undefined,
      job_key: typedJobKey.value || undefined,
      runner_id: filters.value.runner_id || undefined,
      since_ms: sinceMs.value,
      until: oldest.created_at,
      until_id: oldest.id,
      limit: PAGE_SIZE,
    })
    if (asked !== filterGeneration) return
    if (page.length) pages.value = [...pages.value, page]
  } finally {
    loadingOlder.value = false
  }
}

// Any change of filter starts again from the newest rows. Keeping the pages
// would mean showing rows that the new filter excludes.
watch(
  // The debounced key, not the typed one: the reset has to line up with the
  // request it invalidates, or a page fetched under the old filter survives it.
  () => [filters.value.state, typedJobKey.value, filters.value.runner_id, filters.value.window],
  () => {
    filterGeneration += 1
    pages.value = []
  },
)

/** The detail comes out of the list; there is no GET /v1/executions/{id}. */
const selectedId = computed(() => (route.params.id as string | undefined) ?? undefined)
const selected = computed(() => rows.value.find((row) => row.id === selectedId.value) ?? null)

function setFilter(key: 'state' | 'job_key' | 'runner_id' | 'window', value: string) {
  const query = { ...route.query }
  if (value) query[key] = value
  else delete query[key]
  // `replace`, not `push`: typing in a filter should not fill the back button
  // with one entry per keystroke.
  void router.replace({ path: '/executions', query })
}

function clearFilters() {
  void router.replace({ path: '/executions' })
}

function open(row: Execution) {
  void router.push({ path: `/executions/${row.id}`, query: route.query })
}

function close() {
  void router.push({ path: '/executions', query: route.query })
}

const hasFilters = computed(() =>
  Boolean(
    filters.value.state ||
      filters.value.job_key ||
      filters.value.runner_id ||
      filters.value.window,
  ),
)

const STATES = ['queued', 'claimed', 'completed', 'failed', 'dead', 'cancelled']

/**
 * Keyboard navigation — j/k to move, Enter to open, Escape to close.
 *
 * An operations list is read far more often than it is clicked, and this is a
 * tool for people who live in a terminal. `j`/`k` costs one handler and no
 * layout; the React tree has Ctrl+K and nothing else.
 */
const cursor = ref(-1)

watch(rows, (next) => {
  if (cursor.value >= next.length) cursor.value = next.length - 1
})

function onKey(event: KeyboardEvent) {
  const target = event.target as HTMLElement | null
  // Never steal keys from a field someone is typing in.
  if (target && ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName)) return

  if (event.key === 'j' || event.key === 'ArrowDown') {
    cursor.value = Math.min(cursor.value + 1, rows.value.length - 1)
    event.preventDefault()
  } else if (event.key === 'k' || event.key === 'ArrowUp') {
    cursor.value = Math.max(cursor.value - 1, 0)
    event.preventDefault()
  } else if (event.key === 'Enter' && cursor.value >= 0) {
    const row = rows.value[cursor.value]
    if (row) open(row)
  } else if (event.key === 'Escape' && selectedId.value) {
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
    <!-- Filters are controls, not a separate card above the list. The React
         tree used a native <select> here, unstyled and inconsistent with
         everything around it. -->
    <div class="flex flex-wrap items-center gap-2">
      <USelectMenu
        :model-value="filters.state || undefined"
        :items="STATES"
        placeholder="Any state"
        aria-label="Filter by state"
        class="w-40"
        @update:model-value="(value: string) => setFilter('state', value ?? '')"
      />
      <UInput
        :model-value="filters.job_key"
        placeholder="Job key…"
        icon="i-lucide-search"
        aria-label="Filter by job key"
        class="w-56"
        @update:model-value="(value: string) => setFilter('job_key', value)"
      />
      <UInput
        v-if="filters.runner_id"
        :model-value="filters.runner_id"
        readonly
        aria-label="Filtered to one runner"
        icon="i-lucide-cpu"
        class="w-56 font-mono"
      />
      <USelectMenu
        :model-value="filters.window || undefined"
        :items="WINDOWS"
        value-key="value"
        placeholder="Any time"
        aria-label="Filter by time window"
        class="w-44"
        @update:model-value="(value: string) => setFilter('window', value ?? '')"
      />
      <UButton
        v-if="hasFilters"
        variant="ghost"
        color="neutral"
        icon="i-lucide-x"
        @click="clearFilters"
      >
        Clear
      </UButton>

      <span class="cq-num ml-auto text-sm text-muted">{{ rows.length }} runs</span>
    </div>

    <div class="flex min-h-0 flex-1 gap-4">
      <div class="cq-list min-w-0 flex-1">
        <AppLoading
          v-if="isPending && rows.length === 0"
          label="Loading runs"
        />
        <AppError
          v-else-if="isError"
          :error="error"
          :on-retry="() => refetch()"
        />
        <AppEmpty
          v-else-if="rows.length === 0"
          icon="i-lucide-list"
          :title="hasFilters ? 'No runs match these filters' : 'No runs yet'"
          :description="
            hasFilters
              ? 'Nothing in the history matches. Clearing the filters shows everything.'
              : 'Runs appear here as soon as a job fires. Trigger one from its job page to see it.'
          "
        >
          <template
            v-if="hasFilters"
            #action
          >
            <UButton
              variant="subtle"
              color="neutral"
              @click="clearFilters"
            >
              Clear filters
            </UButton>
          </template>
        </AppEmpty>

        <table
          v-else
          class="w-full border-collapse"
        >
          <thead class="sticky top-0 z-10 bg-default">
            <tr class="border-b border-default">
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
                State
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
                Job
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
                Run
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
                Runner
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-right">
                Fired
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-right">
                Duration
              </th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="(row, index) in rows"
              :key="row.id"
              :class="[
                'cq-row',
                'cursor-pointer border-b border-default/60 transition-colors hover:bg-elevated',
                row.id === selectedId && 'bg-elevated',
                index === cursor && row.id !== selectedId && 'ring-1 ring-primary/40 ring-inset',
              ]"
              @click="open(row)"
            >
              <td class="px-[var(--cq-cell-x)]">
                <StatusPill :state="row.state" />
              </td>
              <td class="max-w-[16rem] truncate px-[var(--cq-cell-x)] font-mono text-primary">
                {{ row.job_key }}
              </td>
              <td
                class="cq-num px-[var(--cq-cell-x)] font-mono text-muted"
                :title="row.id"
              >
                {{ shortId(row.id) }}
                <!-- Attempt only when it says something: every run is attempt 1. -->
                <span
                  v-if="row.attempt > 1"
                  class="ml-1 text-warning"
                >#{{ row.attempt }}</span>
              </td>
              <td class="max-w-[14rem] truncate px-[var(--cq-cell-x)] font-mono text-muted">
                {{ row.runner_id ?? '—' }}
              </td>
              <td
                class="cq-num px-[var(--cq-cell-x)] text-right text-muted"
                :title="formatAbsolute(row.fire_at)"
              >
                {{ formatRelative(row.fire_at) }}
              </td>
              <td class="cq-num px-[var(--cq-cell-x)] text-right">
                {{ formatDuration(row.duration_ms) }}
              </td>
            </tr>
          </tbody>
        </table>

        <!--
          Paging back.

          The list was hard-capped at 200 rows, so anything older than that was
          unreachable and a deep link to an older run found nothing. Each click
          fetches one page older than everything on screen and appends it,
          leaving the polled query on the newest page — so the list keeps
          updating after paging, which it did not before.

          At the end it says so, rather than leaving a button that returns
          nothing new.
        -->
        <div
          v-if="rows.length > 0"
          class="flex items-center justify-center gap-3 border-t border-default px-3 py-3"
        >
          <UButton
            v-if="mayHaveMore"
            color="neutral"
            variant="subtle"
            size="xs"
            icon="i-lucide-arrow-down"
            :loading="loadingOlder"
            @click="loadOlder"
          >
            Load older
          </UButton>
          <span
            v-else
            class="text-xs text-muted"
          >That is everything{{ filters.window ? ' in this window' : '' }}.</span>
        </div>
      </div>

      <!-- Only when there is something to show. The pane the audit complained
           about sat empty across 60% of the screen. -->
      <RunDetail
        v-if="selectedId"
        :execution="selected"
        class="w-[26rem] shrink-0"
        @close="close"
      />
    </div>
  </div>
</template>
