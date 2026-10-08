<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useJobRuntime, useJobs } from '~/api/queries'
import type { RuntimeWindow } from '~/api/types'
import { formatDuration } from '~/lib/format'

/**
 * Where the run time went: each job's finished runs in the window, summed,
 * largest first. It replaced a 24-hour throughput chart, which said how many
 * runs there were but not which jobs kept the runners busy.
 *
 * The bars are relative to the largest job, not to the window: the question
 * is which jobs dominate, and a job using a tenth of a day would otherwise
 * be a sliver beside nothing. They fill the row behind the job key rather
 * than sitting in a track of their own, so every bar is measured against the
 * same width whatever the labels beside it say.
 *
 * Each bar has two parts: the time the runs typically waited for a runner,
 * on the left, in the warning tone the live timeline gives a wait, and the
 * time they ran. The wait is the job's median wait times its runs, not the
 * waits summed: one run that came due while the server was down waits for
 * hours, and a sum turned every bar into that one run. The total and the
 * order are run time only — the card asks which jobs keep the runners busy,
 * and waiting is a symptom of that, not load.
 */

const WINDOW_KEY = 'croniq_runtime_window'
const WINDOWS: { label: string; value: RuntimeWindow }[] = [
  { label: 'Last 24 hours', value: '24h' },
  { label: 'Last 7 days', value: '7d' },
  { label: 'Last 30 days', value: '30d' },
]
/** Past this many jobs the rest wait behind "Show all". */
const COLLAPSED = 10

function readWindow(): RuntimeWindow {
  try {
    const stored = localStorage.getItem(WINDOW_KEY)
    return WINDOWS.some((w) => w.value === stored) ? (stored as RuntimeWindow) : '24h'
  } catch {
    return '24h'
  }
}

const span = ref<RuntimeWindow>(readWindow())
watch(span, (value) => {
  try {
    localStorage.setItem(WINDOW_KEY, value)
  } catch {
    // Remembering the choice is a convenience, not a requirement.
  }
})

const { data, isPending } = useJobRuntime(span)
const jobs = computed(() => data.value?.jobs ?? [])
type RuntimeRow = (typeof jobs.value)[number]

/** The typical wait over a job's runs: its median wait, once per run. */
function waited(job: RuntimeRow): number {
  return job.wait_median_ms * job.runs
}

// The widest bar is the longest wait plus run, so both parts share a scale.
const peak = computed(() => Math.max(1, ...jobs.value.map((j) => waited(j) + j.total_ms)))
const total = computed(() => jobs.value.reduce((sum, j) => sum + j.total_ms, 0))
const totalWaited = computed(() => jobs.value.reduce((sum, j) => sum + waited(j), 0))

/** Dispatch priority by job key, for the two non-default levels (#826). */
const { data: jobList } = useJobs()
const priorities = computed(
  () => new Map((jobList.value ?? []).flatMap((j) => (j.priority ? [[j.job_key, j.priority] as const] : []))),
)

const expanded = ref(false)
const shown = computed(() => (expanded.value ? jobs.value : jobs.value.slice(0, COLLAPSED)))

function share(job: { total_ms: number }): string {
  return `${Math.round((job.total_ms / Math.max(1, total.value)) * 100)}%`
}

function rowTooltip(job: RuntimeRow): string {
  const ran = `${share(job)} of the run time in the window`
  return job.wait_median_ms > 0
    ? `${ran} · a run typically waited ${formatDuration(job.wait_median_ms)} for a runner`
    : ran
}

/** Bar geometry: the whole bar against the peak, the wait as part of it. */
function barWidth(job: RuntimeRow): string {
  return `${Math.max(1, ((waited(job) + job.total_ms) / peak.value) * 100)}%`
}
function waitWidth(job: RuntimeRow): string {
  return `${(waited(job) / Math.max(1, waited(job) + job.total_ms)) * 100}%`
}

/** The runs behind a row, over the same window the card is showing. */
function runsLink(jobKey: string, state?: 'failed') {
  return {
    path: '/executions',
    query: { job_key: jobKey, window: span.value, ...(state ? { state } : {}) },
  }
}
</script>

<template>
  <section
    class="rounded-xl border border-default bg-default p-4 shadow-sm"
    data-testid="job-runtime"
  >
    <div class="mb-3 flex flex-wrap items-center justify-between gap-2">
      <p class="cq-label">
        Run time by job
      </p>
      <div class="flex items-center gap-3">
        <span
          v-if="jobs.length"
          v-tooltip="'Finished runs with a recorded duration: time spent running, and before it the time a run typically waits for a runner (each job\'s median wait, once per run). Runs still going, and history removed by retention, are not counted.'"
          class="cq-num flex items-center gap-1.5 text-xs text-muted"
          data-testid="job-runtime-total"
        >
          <span
            v-if="totalWaited > 0"
            class="flex items-center gap-1"
          >
            <span
              class="size-2 rounded-xs bg-warning/40"
              aria-hidden="true"
            />~{{ formatDuration(totalWaited) }} waiting ·
          </span>
          <span class="flex items-center gap-1">
            <span
              class="size-2 rounded-xs bg-primary/30"
              aria-hidden="true"
            />{{ formatDuration(total) }} running
          </span>
        </span>
        <USelectMenu
          v-model="span"
          :items="WINDOWS"
          value-key="value"
          :search-input="false"
          size="xs"
          class="w-36"
          aria-label="Time window"
          data-testid="job-runtime-window"
        />
      </div>
    </div>
    <AppEmpty
      v-if="!isPending && jobs.length === 0"
      size="tight"
      icon="i-lucide-timer"
      title="No finished runs in the window"
    />
    <!-- One grid for the whole list, each row a subgrid of it, so the
         figures line up in columns whatever their length: ragged badges were
         what made the bar version read as restless. -->
    <ul
      v-else
      class="grid grid-cols-[minmax(0,1fr)_auto_auto_auto] gap-y-1"
    >
      <li
        v-for="job in shown"
        :key="job.job_key"
        v-tooltip="rowTooltip(job)"
        class="relative col-span-4 grid h-8 grid-cols-subgrid items-center gap-x-2 overflow-hidden rounded-md px-2 text-sm"
        data-testid="job-runtime-row"
      >
        <!-- The row's background is the bar: one width, so every bar is
             measured against the same length. The wait comes first, as it
             does in time. -->
        <div
          class="absolute inset-y-0 left-0 flex overflow-hidden rounded-md"
          :style="{ width: barWidth(job) }"
          aria-hidden="true"
          data-testid="job-runtime-bar"
        >
          <div
            v-if="job.wait_median_ms > 0"
            class="h-full shrink-0 bg-warning/20 dark:bg-warning/25"
            :style="{ width: waitWidth(job) }"
            data-testid="job-runtime-wait"
          />
          <div class="h-full flex-1 bg-primary/12 dark:bg-primary/20" />
        </div>
        <span class="relative flex min-w-0 items-center gap-1.5">
          <RouterLink
            :to="`/jobs/${encodeURIComponent(job.job_key)}`"
            class="truncate font-mono text-primary hover:underline"
          >
            {{ job.job_key }}
          </RouterLink>
          <!-- Dispatch priority (#826), only when it is not the default. -->
          <UBadge
            v-if="priorities.get(job.job_key)"
            v-tooltip="`Dispatch priority: ${priorities.get(job.job_key)}`"
            size="sm"
            variant="subtle"
            :color="priorities.get(job.job_key) === 'high' ? 'primary' : 'neutral'"
            :label="priorities.get(job.job_key)"
            class="shrink-0"
            data-testid="job-runtime-priority"
          />
        </span>
        <RouterLink
          v-if="job.failed"
          :to="runsLink(job.job_key, 'failed')"
          class="relative justify-self-end"
          :aria-label="`${job.failed} failed runs of ${job.job_key}`"
        >
          <UBadge
            color="error"
            variant="subtle"
            size="sm"
            class="cq-num"
          >
            {{ job.failed }} failed
          </UBadge>
        </RouterLink>
        <span v-else />
        <RouterLink
          :to="runsLink(job.job_key)"
          class="relative justify-self-end"
          :aria-label="`${job.runs} runs of ${job.job_key}`"
        >
          <UBadge
            color="neutral"
            variant="subtle"
            size="sm"
            class="cq-num"
          >
            {{ job.runs }} run{{ job.runs === 1 ? '' : 's' }}
          </UBadge>
        </RouterLink>
        <span class="cq-num relative min-w-[4.5rem] text-right font-medium text-highlighted">
          {{ formatDuration(job.total_ms) }}
          <span class="sr-only">, {{ share(job) }} of the run time in the window</span>
        </span>
      </li>
    </ul>
    <UButton
      v-if="jobs.length > COLLAPSED"
      class="mt-2"
      size="xs"
      variant="ghost"
      color="neutral"
      :label="expanded ? 'Show fewer' : `Show all ${jobs.length}`"
      :icon="expanded ? 'i-lucide-chevron-up' : 'i-lucide-chevron-down'"
      @click="expanded = !expanded"
    />
  </section>
</template>
