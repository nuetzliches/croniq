<script setup lang="ts">
import { computed } from 'vue'
import { useForecast, useJobStates, useJobs } from '~/api/queries'
import { formatAbsolute, formatRelative } from '~/lib/format'

/**
 * What fires next.
 *
 * This is the one genuinely new thing in the rebuild, and it comes from the
 * audit's sharpest finding: croniq is a *scheduler* that shows the past on
 * every screen and the future on none. "NEXT FIRE in 33s" existed exactly
 * once, buried in one job's detail.
 *
 * It needed no server work. `/v1/dashboard/forecast` has been there all along
 * and the React dashboard simply never called it — only the job detail did.
 * The data was not missing, the question was.
 *
 * Two sources, deliberately:
 *
 * `useJobStates` gives the exact next fire per job, which is what an operator
 * actually reads — "demo:report, in 4 min". `useForecast` gives the shape of
 * the hour ahead in buckets, which is what says "and then it gets busy". A
 * list alone hides the load; a histogram alone hides the names.
 */
const { data: states, isPending } = useJobStates()
const { data: forecast } = useForecast(60, 5)
const { data: jobs } = useJobs()

/**
 * Only jobs that still exist.
 *
 * `GET /v1/jobs/states` outlives the job. That is deliberate on the server's
 * side and it says so at boot: "job_states rows exist for jobs this
 * configuration does not define. They are kept (a job may be temporarily
 * absent) and no longer produce metrics" — a job pulled out of the Croniqfile
 * and put back should not lose its history (issue #470).
 *
 * Which means a state row is not evidence that a job exists, and this panel
 * read it as though it were. The result was that deleted jobs sat in "what
 * fires next" forever, with a next fire time that nothing would ever honour.
 * The jobs list never had the bug because it builds from `/v1/jobs` and joins
 * state onto it; this built from state and joined nothing.
 */
const liveKeys = computed(() => new Set((jobs.value ?? []).map((job) => job.job_key)))
const liveStates = computed(() =>
  // Before the job list arrives, show nothing rather than everything: a brief
  // empty rail is better than one that flashes jobs that are gone.
  jobs.value ? (states.value ?? []).filter((state) => liveKeys.value.has(state.job_key)) : [],
)

/** The soonest handful, nearest first. Jobs with no next fire are not due. */
const upcoming = computed(() => {
  const rows = liveStates.value
    .filter((state) => state.next_fire_at && state.status === 'active')
    .sort((a, b) => Date.parse(a.next_fire_at!) - Date.parse(b.next_fire_at!))
  return rows.slice(0, 6)
})

/**
 * Overdue jobs, surfaced separately and above.
 *
 * A job that should have fired and did not is the most urgent thing this
 * component knows, and burying it inside a chronological list would be the
 * wrong emphasis — it is not "upcoming", it is late.
 */
const overdue = computed(() => liveStates.value.filter((state) => state.overdue))

const buckets = computed(() => forecast.value?.buckets ?? [])
const peak = computed(() => Math.max(1, ...buckets.value.map((bucket) => bucket.count)))
const totalAhead = computed(() =>
  buckets.value.reduce((sum, bucket) => sum + bucket.count, 0),
)
</script>

<template>
  <section class="rounded-xl border border-default bg-default p-4 shadow-sm">
    <div class="mb-3 flex items-baseline justify-between gap-3">
      <p class="cq-label">
        Next hour
      </p>
      <span class="cq-num text-xs text-muted">{{ totalAhead }} fires</span>
    </div>

    <AppLoading
      v-if="isPending"
      size="tight"
      label="Loading the schedule"
    />

    <template v-else>
      <!-- Late first. It is not upcoming, it is overdue. -->
      <RouterLink
        v-if="overdue.length"
        to="/jobs"
        class="mb-3 flex items-center gap-2 rounded-lg border border-warning/40 bg-warning/10 px-3 py-2 text-sm text-warning"
      >
        <UIcon
          name="i-lucide-clock-alert"
          class="size-4 shrink-0"
        />
        <span class="min-w-0 flex-1 truncate">
          {{ overdue.length }} job{{ overdue.length === 1 ? '' : 's' }} overdue —
          {{ overdue.map((state) => state.job_key).join(', ') }}
        </span>
      </RouterLink>

      <!--
        The histogram. Bars rather than a line: the buckets are discrete
        five-minute windows, and a line between them would imply a rate that
        the scheduler does not have.
      -->
      <div
        v-if="buckets.length"
        class="mb-4 flex h-12 items-end gap-0.5"
        role="img"
        :aria-label="`${totalAhead} runs due in the next hour`"
      >
        <div
          v-for="bucket in buckets"
          :key="bucket.start"
          class="min-w-0 flex-1 rounded-t-sm transition-colors"
          :class="bucket.count ? 'bg-primary/70' : 'bg-elevated'"
          :style="{ height: `${Math.max(bucket.count ? 8 : 3, (bucket.count / peak) * 100)}%` }"
          :title="`${formatAbsolute(bucket.start)} — ${bucket.count} run${bucket.count === 1 ? '' : 's'}${bucket.jobs.length ? `: ${bucket.jobs.join(', ')}` : ''}`"
        />
      </div>

      <AppEmpty
        v-if="upcoming.length === 0"
        size="tight"
        icon="i-lucide-calendar-off"
        title="Nothing scheduled"
        description="No active job has a next fire time. Triggers may be disabled, or every job is manual."
      />

      <ul
        v-else
        class="flex flex-col"
      >
        <li
          v-for="state in upcoming"
          :key="state.job_key"
          class="flex h-8 items-center gap-3 text-sm"
        >
          <span class="min-w-0 flex-1 truncate font-mono text-primary">{{ state.job_key }}</span>
          <span
            class="cq-num text-muted"
            :title="formatAbsolute(state.next_fire_at)"
          >{{ formatRelative(state.next_fire_at) }}</span>
        </li>
      </ul>
    </template>
  </section>
</template>
