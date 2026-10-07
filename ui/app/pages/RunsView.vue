<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { fetchExecutions, useExecutions, useNotes } from '~/api/queries'
import type { Execution } from '~/api/types'
import { formatDuration, shortId, stateLabel } from '~/lib/format'
import { useDebounced } from '~/composables/useDebounced'
import { useFavorites } from '~/composables/useFavorites'
import { useTimeDisplay } from '~/composables/useTimeDisplay'
import { FAILURE_STATES, nextFailureIndex } from '~/lib/next-failure'
import { notesByExecution } from '~/lib/notes'

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
  /**
   * The typed search, matched as a case-insensitive substring of the job key
   * — the same thing the box on Jobs does.
   *
   * It is deliberately not `job_key`. That one is exact and stays exact:
   * `/executions?job_key=mail:send` is what a job's detail links to for "its
   * runs", and a substring there would answer with `mail:send-retry`'s runs
   * as well (issue #753). So a deep link keeps meaning one job, and the box
   * gets its own parameter.
   */
  q: (route.query.q as string) || '',
  job_key: (route.query.job_key as string) || '',
  runner_id: (route.query.runner_id as string) || '',
  window: (route.query.window as string) || '',
  /**
   * `1`: only runs of the jobs this user has starred. The server resolves the
   * set, so paging and the row count stay true to the filter.
   */
  favorites: route.query.favorites === '1' ? '1' : '',
}))

const { favorites, available: favoritesAvailable } = useFavorites()

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
 * The search is the only one typed a character at a time; `state` and `window`
 * are chosen from a menu, and `job_key` and `runner_id` arrive from a link, so
 * those stay immediate (issue #730).
 */
const typedSearch = useDebounced(() => filters.value.q)

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
  job_key: filters.value.job_key || undefined,
  job_key_contains: typedSearch.value || undefined,
  runner_id: filters.value.runner_id || undefined,
  favorites: filters.value.favorites === '1',
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

/**
 * Notes on the rows on screen, in one request. Capped at the server's 200 ids,
 * which is one page: rows from "Load older" go without badges rather than
 * multiplying requests, and their notes are a click away in the detail.
 */
const { data: rowNotes } = useNotes(() => ({
  execution_ids: rows.value.slice(0, 200).map((row) => row.id),
  limit: 500,
}))
const notesByRun = computed(() => notesByExecution(rowNotes.value ?? []))

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
      job_key: filters.value.job_key || undefined,
      job_key_contains: typedSearch.value || undefined,
      runner_id: filters.value.runner_id || undefined,
      favorites: filters.value.favorites === '1',
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
  () => [
    filters.value.state,
    typedSearch.value,
    filters.value.job_key,
    filters.value.runner_id,
    filters.value.window,
    filters.value.favorites,
  ],
  () => {
    filterGeneration += 1
    pages.value = []
  },
)

/** The detail comes out of the list; there is no GET /v1/executions/{id}. */
const selectedId = computed(() => (route.params.id as string | undefined) ?? undefined)
const selected = computed(() => rows.value.find((row) => row.id === selectedId.value) ?? null)

function setFilter(
  key: 'state' | 'q' | 'job_key' | 'runner_id' | 'window' | 'favorites',
  value: string,
) {
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

/**
 * The exact filters a link brought in, shown as chips beside the controls.
 *
 * They used to be read-only inputs, which looked like a second search box with
 * no label, truncated the key at the input's width, and could only be removed
 * along with every other filter. A chip says what it is, sizes to the key, and
 * comes off on its own — the way back from one job's runs to the full list.
 */
const pinned = computed(() =>
  [
    { key: 'job_key' as const, label: 'Job', noun: 'job', icon: 'i-lucide-clock', value: filters.value.job_key },
    { key: 'runner_id' as const, label: 'Runner', noun: 'runner', icon: 'i-lucide-cpu', value: filters.value.runner_id },
  ].filter((chip) => chip.value),
)

const hasFilters = computed(() =>
  Boolean(
    filters.value.state ||
      filters.value.q ||
      filters.value.job_key ||
      filters.value.runner_id ||
      filters.value.window ||
      filters.value.favorites,
  ),
)

/** "Favorites only" with nothing starred is a different empty from a filter that misses. */
const noFavoritesYet = computed(
  () => filters.value.favorites === '1' && favoritesAvailable.value && favorites.value.size === 0,
)

// Labelled through `stateLabel` so the filter says what the pill says; the
// value stays the store's name, because that is what the API filters on.
// The URL and the API both carry the selection as a comma-separated list
// (`?state=queued,claimed`); the menu works on an array.
const selectedStates = computed(() => (filters.value.state ? filters.value.state.split(',') : []))

const STATES = ['queued', 'claimed', 'completed', 'failed', 'dead', 'cancelled'].map((value) => ({
  label: stateLabel(value),
  value,
}))

/** "Fired" as "3 min ago" or as the clock time — see `useTimeDisplay`. */
const fired = useTimeDisplay()

/**
 * Keyboard navigation — j/k to move, Enter to open, Escape to close. (`t`, for
 * how "Fired" reads, is every time column's, in `TimeHeading`.)
 *
 * An operations list is read far more often than it is clicked, and this is a
 * tool for people who live in a terminal. `j`/`k` costs one handler and no
 * layout; the React tree has Ctrl+K and nothing else.
 */
const cursor = ref(-1)

watch(rows, (next) => {
  if (cursor.value >= next.length) cursor.value = next.length - 1
})

/**
 * "Next failure" — scroll to the next failed or dead run below the cursor.
 *
 * Without a state filter the failures sit among hundreds of green rows, and
 * finding them meant scanning for red pills. With one, every row is already
 * the state asked for, so the button only shows when there is no state filter.
 *
 * It moves the j/k cursor rather than keeping a position of its own, so Enter
 * opens the run it landed on and j/k carry on from there. It counts the rows
 * on screen: a failure further back than the loaded pages needs "Load older"
 * first.
 */
const listEl = ref<HTMLElement | null>(null)

const failureCount = computed(
  () => rows.value.filter((row) => FAILURE_STATES.has(row.state)).length,
)
const showNextFailure = computed(() => !filters.value.state && failureCount.value > 0)

function scrollToNextFailure() {
  const index = nextFailureIndex(
    rows.value.map((row) => row.state),
    cursor.value,
  )
  if (index < 0) return
  cursor.value = index
  const reduced = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches
  // `center`, not `start`: the sticky header would cover a row scrolled to the top.
  listEl.value
    ?.querySelector(`[data-row-index="${index}"]`)
    ?.scrollIntoView({ block: 'center', behavior: reduced ? 'auto' : 'smooth' })
}

function onKey(event: KeyboardEvent) {
  const target = event.target as HTMLElement | null
  // Never steal keys from a field someone is typing in.
  if (target && ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName)) return
  // Nor Enter from a focused button or link: that Enter is its click, and
  // opening the cursor row as well would act twice.
  if (event.key === 'Enter' && target && ['BUTTON', 'A'].includes(target.tagName)) return

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
        :model-value="selectedStates"
        :items="STATES"
        value-key="value"
        multiple
        placeholder="Any state"
        aria-label="Filter by state"
        clear
        class="w-48"
        @update:model-value="(value: string[]) => setFilter('state', (value ?? []).join(','))"
      />
      <UInput
        :model-value="filters.q"
        placeholder="Search job key…"
        icon="i-lucide-search"
        aria-label="Search by job key"
        class="w-56"
        @update:model-value="(value: string) => setFilter('q', value)"
      />
      <!-- Exact, unlike the box: a chip names one job or one runner, and
           widening the list is what the box is for (issue #753). -->
      <div
        v-for="chip in pinned"
        :key="chip.key"
        role="group"
        :aria-label="`Filtered to one ${chip.noun}`"
        class="inline-flex h-8 max-w-full items-center gap-1.5 rounded-md border border-primary/30 bg-primary/10 pr-0.5 pl-2 text-sm"
      >
        <UIcon
          :name="chip.icon"
          class="size-4 shrink-0 text-primary"
        />
        <span class="text-muted">{{ chip.label }}</span>
        <span
          class="max-w-[24rem] truncate font-mono text-highlighted"
          :title="chip.value"
        >{{ chip.value }}</span>
        <UButton
          variant="ghost"
          color="neutral"
          size="xs"
          icon="i-lucide-x"
          :aria-label="`Remove ${chip.noun} filter`"
          @click="setFilter(chip.key, '')"
        />
      </div>
      <USelectMenu
        :model-value="filters.window || undefined"
        :items="WINDOWS"
        value-key="value"
        placeholder="Any time"
        aria-label="Filter by time window"
        clear
        class="w-44"
        @update:model-value="(value: string) => setFilter('window', value ?? '')"
      />
      <UButton
        v-if="favoritesAvailable || filters.favorites"
        icon="i-lucide-star"
        :variant="filters.favorites ? 'subtle' : 'ghost'"
        color="neutral"
        :aria-pressed="Boolean(filters.favorites)"
        title="Show only runs of the jobs you have starred"
        data-testid="runs-favorites-only"
        @click="setFilter('favorites', filters.favorites ? '' : '1')"
      >
        Favorites only
      </UButton>
      <UButton
        v-if="showNextFailure"
        color="error"
        variant="subtle"
        icon="i-lucide-arrow-down-to-line"
        title="Scroll to the next failed run"
        @click="scrollToNextFailure"
      >
        Next failure <span class="cq-num">· {{ failureCount }}</span>
      </UButton>
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
      <div
        ref="listEl"
        class="cq-list min-w-0 flex-1"
      >
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
          :title="
            noFavoritesYet
              ? 'No favorite jobs yet'
              : hasFilters
                ? 'No runs match these filters'
                : 'No runs yet'
          "
          :description="
            noFavoritesYet
              ? 'Star a job on the Jobs screen or in its detail, and its runs show up here.'
              : hasFilters
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
                <TimeHeading label="Fired" />
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
              :data-row-index="index"
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
              <td class="max-w-[20rem] px-[var(--cq-cell-x)] font-mono text-primary">
                <div class="flex min-w-0 items-center gap-2">
                  <span class="truncate">{{ row.job_key }}</span>
                  <NoteBadge :notes="notesByRun.get(row.id)" />
                </div>
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
                :title="fired.title(row.fire_at)"
              >
                {{ fired.text(row.fire_at) }}
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
