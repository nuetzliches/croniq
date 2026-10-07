<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  useCreateNote,
  useDeadLetterCount,
  useExecutions,
  useFailureHeatmap,
  useHealth,
  useJobs,
  useNotes,
  useRunners,
  useThroughput,
} from '~/api/queries'
import type { Execution } from '~/api/types'
import { useActionError } from '~/composables/useActionError'
import { useCanWriteNotes } from '~/composables/useCanWriteNotes'
import { useExecutionsStream } from '~/composables/useExecutionsStream'
import { formatAbsolute, formatDuration, formatRelative } from '~/lib/format'
import { ACTIVE_CAP, countActive } from '~/lib/live-timeline'
import { notesByExecution } from '~/lib/notes'

/**
 * The landing page: is anything wrong, and what happens next.
 *
 * The decision from docs/ui-screen-inventory.md is a status board plus a
 * failures-only excerpt — deliberately *not* the shipping dashboard's general
 * "recent executions" list, which would be a fourth rendering of the runs
 * table. What replaces it is the live timeline: what runs now, and — in its
 * lane labels and header — what fires next, the question the "next hour"
 * rail was added to answer before the timeline absorbed it.
 */
const { data: health } = useHealth()
const { data: jobs } = useJobs()
const { data: runners } = useRunners()
/** The queue, not the page `useDeadLetters` returns (issue #722). */
const deadLetterCount = useDeadLetterCount()
const { data: throughput } = useThroughput('24h')
const { data: heatmap } = useFailureHeatmap(7)

/** Rows the card shows. */
const FAILURES_SHOWN = 5
/**
 * Rows it fetches, so that with checked failures hidden there are still five
 * to show. Well inside the notes endpoint's 200 ids per request.
 */
const FAILURES_FETCHED = 50
const HIDE_CHECKED_KEY = 'croniq_failures_hide_checked'

/** Only failures. A general run list belongs on /executions, once. */
const { data: recentFailures } = useExecutions(() => ({
  state: 'failed',
  limit: FAILURES_FETCHED,
}))

/**
 * Who has already looked at each failure. One request for all the rows, and
 * a one-click "checked" beside each, so acknowledging a failure does not mean
 * opening it — opening it is for when there is something to say.
 */
const { data: failureNotes } = useNotes(() => ({
  execution_ids: (recentFailures.value ?? []).map((run) => run.id),
}))
const notesByRun = computed(() => notesByExecution(failureNotes.value ?? []))

function readHideChecked(): boolean {
  try {
    return localStorage.getItem(HIDE_CHECKED_KEY) === '1'
  } catch {
    return false
  }
}

/**
 * "Hide checked": a failure someone has marked as checked has been dealt
 * with, and on a busy day it pushes the ones nobody has looked at off the
 * card. Off by default, so the card still answers "what failed last" until
 * an operator asks it to answer "what is still open". Remembered per browser.
 */
const hideChecked = ref(readHideChecked())
watch(hideChecked, (value) => {
  try {
    localStorage.setItem(HIDE_CHECKED_KEY, value ? '1' : '0')
  } catch {
    // A convenience, not a requirement.
  }
})

const isChecked = (run: Execution) =>
  (notesByRun.value.get(run.id) ?? []).some((note) => note.kind === 'ack')

const failures = computed(() => {
  const all = recentFailures.value ?? []
  // Until the notes have loaded nothing is known to be checked; filtering
  // then would show checked rows and pull them a moment later.
  const open = hideChecked.value && failureNotes.value ? all.filter((run) => !isChecked(run)) : all
  return open.slice(0, FAILURES_SHOWN)
})
const hiddenCount = computed(() =>
  hideChecked.value ? (recentFailures.value ?? []).filter(isChecked).length : 0,
)
const canWriteNotes = useCanWriteNotes()
const createNote = useCreateNote()
const { error: ackError, attempt: attemptAck } = useActionError()
const acking = ref<string | null>(null)

async function markChecked(run: Execution) {
  acking.value = run.id
  await attemptAck(() =>
    createNote.mutateAsync({ job_key: run.job_key, execution_id: run.id, kind: 'ack' }),
  )
  acking.value = null
}

/**
 * "Queue depth" and "Running" from the timeline's own stream, so the cards
 * and the timeline below them move together. `/health` has the figures too,
 * but polled every 5 s and from other sources: the in-memory dispatch queue,
 * and what each runner last reported in flight (a poll behind, and counting
 * ephemeral runs, which have no row and no bar). The stream's are the store's
 * queued and claimed rows, which is also what the cards link to.
 *
 * Counted live and unfiltered, whatever the timeline is paused at or filtered
 * by. `/health` stands in while the stream has nothing to say: not yet
 * connected, reconnecting, not allowed (no `executions:read`), or at its cap.
 */
const stream = useExecutionsStream()
const liveCounts = computed(() => {
  if (!stream.received.value || !stream.connected.value || stream.unavailable.value) return null
  const counts = countActive(stream.runs.value)
  return counts.running + counts.queued >= ACTIVE_CAP ? null : counts
})
const queuedNow = computed(() => liveCounts.value?.queued ?? health.value?.queued)
const runningNow = computed(() => liveCounts.value?.running ?? health.value?.running)

const activeJobs = computed(() => (jobs.value ?? []).filter((job) => job.is_active).length)
const online = computed(
  () => (runners.value ?? []).filter((runner) => runner.status === 'online').length,
)
/**
 * Slots the online runners offer, so "Running" reads against capacity: a
 * queue that grows while every slot is busy wants more runners, one that
 * grows with slots free points at a guard (concurrency, capabilities).
 */
const slots = computed(() =>
  (runners.value ?? [])
    .filter((runner) => runner.status === 'online')
    .reduce((sum, runner) => sum + runner.max_inflight, 0),
)

const buckets = computed(() => throughput.value?.buckets ?? [])
const totals = computed(() =>
  buckets.value.reduce(
    (sum, bucket) => ({ ok: sum.ok + bucket.ok, err: sum.err + bucket.err }),
    { ok: 0, err: 0 },
  ),
)
const successRate = computed(() => {
  const { ok, err } = totals.value
  const total = ok + err
  return total === 0 ? null : (ok / total) * 100
})

/**
 * The heatmap the audit wanted promoted: it answers "is something wrong"
 * better than any single number, and in the React tree it is small,
 * unlabelled, and in the bottom-right corner.
 */
const heatRows = computed(() => heatmap.value?.rows ?? [])
const heatMax = computed(() => Math.max(1, ...heatRows.value.flat()))
const WEEKDAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']
</script>

<template>
  <div class="flex flex-col gap-4">
    <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-5">
      <KpiCard
        label="Queue depth"
        :value="queuedNow ?? '—'"
        :sub="`${activeJobs} active job${activeJobs === 1 ? '' : 's'}`"
        :tone="(queuedNow ?? 0) > 0 ? 'warning' : 'default'"
        to="/executions?state=queued"
      />
      <!-- The claimed rows, as on the timeline (see `liveCounts`). Running
           work is normal, so no tone. -->
      <KpiCard
        label="Running"
        :value="runningNow ?? '—'"
        :sub="slots ? `of ${slots} slot${slots === 1 ? '' : 's'}` : 'no runner capacity'"
        to="/executions?state=claimed"
      />
      <KpiCard
        label="Runners online"
        :value="online"
        :sub="
          health?.runners_stale
            ? `${health.runners_stale} stale, ${health.runners_dead ?? 0} dead`
            : 'all healthy'
        "
        :tone="online === 0 ? 'error' : health?.runners_stale ? 'warning' : 'success'"
        to="/runners"
      />
      <KpiCard
        label="Success rate (24h)"
        :value="successRate === null ? '—' : `${successRate.toFixed(1)}%`"
        :sub="`${totals.ok} ok · ${totals.err} failed`"
        :tone="successRate === null ? 'default' : successRate < 95 ? 'error' : 'success'"
        to="/executions?state=failed&window=24h"
      />
      <!-- The count, not the length of a page. `useDeadLetters` is capped at
           the server's default of 50, so a queue of any size above that used to
           read as "50" on the screen an operator looks at first (issue #722).
           #692 moved the other two surfaces onto the count endpoint; this tile
           was missed. -->
      <KpiCard
        label="Dead letters"
        :value="deadLetterCount"
        :sub="deadLetterCount ? 'waiting for a decision' : 'none pending'"
        :tone="deadLetterCount ? 'error' : 'success'"
        to="/dead-letters"
      />
    </div>

    <!-- What is running now and what fires next, full width: a timeline
         reads by its length, and it absorbed the "next hour" rail. -->
    <LiveTimeline :stream="stream" />

    <div class="grid gap-4 lg:grid-cols-[1fr_22rem]">
      <div class="flex flex-col gap-4">
        <!-- Where the run time went, per job. It replaced a 24-hour
             throughput chart; the success rate above still reads the
             throughput totals. -->
        <JobRuntimeCard />

        <!-- Failures only. Not a general run list: that is /executions. -->
        <section class="rounded-xl border border-default bg-default p-4 shadow-sm">
          <div class="mb-3 flex items-center justify-between gap-3">
            <p class="cq-label">
              Recent failures
            </p>
            <div class="flex items-center gap-4">
              <USwitch
                v-model="hideChecked"
                size="xs"
                label="Hide checked"
                data-testid="failures-hide-checked"
                :ui="{ label: 'text-xs font-normal text-muted' }"
              />
              <RouterLink
                to="/executions?state=failed"
                class="text-xs text-primary hover:underline"
              >
                View all
              </RouterLink>
            </div>
          </div>
          <AppEmpty
            v-if="!failures.length && hiddenCount"
            size="tight"
            icon="i-lucide-check-check"
            title="Every recent failure is checked"
            :description="`${hiddenCount} checked failure${hiddenCount === 1 ? ' is' : 's are'} hidden.`"
          />
          <AppEmpty
            v-else-if="!failures.length"
            size="tight"
            icon="i-lucide-check"
            title="Nothing has failed"
            description="The most recent runs all completed."
          />
          <ul
            v-else
            class="flex flex-col"
          >
            <li
              v-for="run in failures"
              :key="run.id"
              class="flex h-9 items-center gap-3 text-sm"
            >
              <StatusPill :state="run.state" />
              <RouterLink
                :to="`/executions/${run.id}`"
                class="min-w-0 flex-1 truncate font-mono text-primary hover:underline"
              >
                {{ run.job_key }}
              </RouterLink>
              <NoteBadge :notes="notesByRun.get(run.id)" />
              <span
                v-tooltip="run.error"
                class="hidden min-w-0 max-w-[18rem] truncate text-xs text-muted md:block"
              >{{ run.error ?? '' }}</span>
              <span
                v-tooltip="formatAbsolute(run.fire_at)"
                class="cq-num text-xs text-muted"
              >{{ formatRelative(run.fire_at) }}</span>
              <span class="cq-num w-16 text-right text-xs text-muted">{{
                formatDuration(run.duration_ms)
              }}</span>
              <UButton
                v-if="canWriteNotes"
                v-tooltip="'Mark as checked'"
                icon="i-lucide-check"
                color="neutral"
                variant="ghost"
                size="xs"
                :loading="acking === run.id"
                :aria-label="`Mark the ${run.job_key} failure as checked`"
                @click="markChecked(run)"
              />
            </li>
          </ul>
          <UAlert
            v-if="ackError"
            class="mt-2"
            color="error"
            variant="subtle"
            icon="i-lucide-alert-triangle"
            :description="ackError"
            role="alert"
            close
            @update:open="ackError = null"
          />
        </section>
      </div>

      <div class="flex flex-col gap-4">
        <!-- Promoted out of the bottom-right corner, and labelled. It answers
             "is something wrong" better than any number here. -->
        <section class="rounded-xl border border-default bg-default p-4 shadow-sm">
          <p class="cq-label mb-3">
            Failures · last 7 days
          </p>
          <AppEmpty
            v-if="heatRows.length === 0"
            size="tight"
            icon="i-lucide-grid-3x3"
            title="No data yet"
          />
          <div
            v-else
            class="flex flex-col gap-1"
          >
            <div
              v-for="(row, dayIndex) in heatRows"
              :key="dayIndex"
              class="flex items-center gap-1.5"
            >
              <span class="cq-label w-8 shrink-0 normal-case">{{ WEEKDAYS[dayIndex] }}</span>
              <div class="flex min-w-0 flex-1 gap-px">
                <div
                  v-for="(count, hour) in row"
                  :key="hour"
                  v-tooltip="`${WEEKDAYS[dayIndex]} ${String(hour).padStart(2, '0')}:00 — ${count} failure${count === 1 ? '' : 's'}`"
                  class="h-3 min-w-0 flex-1 rounded-[2px]"
                  :class="count ? 'bg-error' : 'bg-elevated'"
                  :style="count ? { opacity: 0.35 + (count / heatMax) * 0.65 } : undefined"
                />
              </div>
            </div>
            <div class="mt-1 flex justify-between">
              <span class="cq-label normal-case">00:00</span>
              <span class="cq-label normal-case">23:00</span>
            </div>
          </div>
        </section>
      </div>
    </div>
  </div>
</template>
