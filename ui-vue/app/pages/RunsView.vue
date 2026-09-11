<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useExecutions } from '~/api/queries'
import type { Execution } from '~/api/types'
import { formatAbsolute, formatDuration, formatRelative, shortId } from '~/lib/format'
import { useUiStore } from '~/stores/ui'

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
const ui = useUiStore()

/**
 * Filters live in the URL, not in component state. That is the contract the
 * Playwright suite asserts and the reason a filtered view can be pasted into a
 * ticket.
 */
const filters = computed(() => ({
  state: (route.query.state as string) || '',
  job_key: (route.query.job_key as string) || '',
  runner_id: (route.query.runner_id as string) || '',
}))

// A getter, not a value — see useExecutions. Passing `filters.value` here is
// the mistake that makes the list freeze on its first filter.
const { data, isPending, isError, error, refetch } = useExecutions(() => ({
  state: filters.value.state || undefined,
  job_key: filters.value.job_key || undefined,
  runner_id: filters.value.runner_id || undefined,
}))

const rows = computed<Execution[]>(() => data.value ?? [])

/** The detail comes out of the list; there is no GET /v1/executions/{id}. */
const selectedId = computed(() => (route.params.id as string | undefined) ?? undefined)
const selected = computed(() => rows.value.find((row) => row.id === selectedId.value) ?? null)

function setFilter(key: 'state' | 'job_key' | 'runner_id', value: string) {
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
  Boolean(filters.value.state || filters.value.job_key || filters.value.runner_id),
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

const rowClass = computed(() =>
  ui.compact ? 'h-8 text-xs' : 'h-[var(--cq-row-h)] text-sm',
)
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
      <UButton
        v-if="hasFilters"
        variant="ghost"
        color="neutral"
        icon="i-lucide-x"
        @click="clearFilters"
      >
        Clear
      </UButton>

      <div class="ml-auto flex items-center gap-2">
        <span class="cq-num text-sm text-muted">{{ rows.length }} runs</span>
        <UButton
          color="neutral"
          variant="ghost"
          :icon="ui.compact ? 'i-lucide-rows-3' : 'i-lucide-rows-4'"
          :aria-label="ui.compact ? 'Comfortable rows' : 'Compact rows'"
          :title="ui.compact ? 'Comfortable rows' : 'Compact rows'"
          @click="ui.toggleCompact()"
        />
      </div>
    </div>

    <div class="flex min-h-0 flex-1 gap-4">
      <div class="min-w-0 flex-1 overflow-auto rounded-lg border border-default">
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
                rowClass,
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
