<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useJobRuntime } from '~/api/queries'
import type { RuntimeWindow } from '~/api/types'
import { formatDuration } from '~/lib/format'

/**
 * Where the run time went: each job's finished runs in the window, summed,
 * largest first. It replaced a 24-hour throughput chart, which said how many
 * runs there were but not which jobs kept the runners busy.
 *
 * The bars are relative to the largest job, not to the window: the question
 * is which jobs dominate, and a job using a tenth of a day would otherwise
 * be a sliver beside nothing.
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
const peak = computed(() => Math.max(1, ...jobs.value.map((j) => j.total_ms)))
const total = computed(() => jobs.value.reduce((sum, j) => sum + j.total_ms, 0))

const expanded = ref(false)
const shown = computed(() => (expanded.value ? jobs.value : jobs.value.slice(0, COLLAPSED)))
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
          class="cq-num text-xs text-muted"
          title="Finished runs with a recorded duration. Runs still going, and history removed by retention, are not counted."
        >{{ formatDuration(total) }} total</span>
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
    <ul
      v-else
      class="flex flex-col gap-1.5"
    >
      <li
        v-for="job in shown"
        :key="job.job_key"
        class="grid grid-cols-[minmax(0,14rem)_1fr_auto] items-center gap-3 text-sm"
        data-testid="job-runtime-row"
      >
        <RouterLink
          :to="`/jobs/${encodeURIComponent(job.job_key)}`"
          class="truncate font-mono text-primary hover:underline"
          :title="job.job_key"
        >
          {{ job.job_key }}
        </RouterLink>
        <div
          class="h-2 min-w-0 overflow-hidden rounded-full bg-elevated"
          role="img"
          :aria-label="`${Math.round((job.total_ms / total) * 100)}% of the run time in the window`"
        >
          <div
            class="h-full rounded-full bg-primary/70"
            :style="{ width: `${Math.max(1, (job.total_ms / peak) * 100)}%` }"
          />
        </div>
        <span class="cq-num flex items-baseline justify-end gap-2 whitespace-nowrap">
          <span class="text-highlighted">{{ formatDuration(job.total_ms) }}</span>
          <span class="min-w-16 text-right text-xs text-muted">
            {{ job.runs }} run{{ job.runs === 1 ? '' : 's' }}<span
              v-if="job.failed"
              class="text-error"
            > · {{ job.failed }} failed</span>
          </span>
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
