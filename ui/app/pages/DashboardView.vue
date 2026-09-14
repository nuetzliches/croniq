<script setup lang="ts">
import { computed } from 'vue'
import {
  useDeadLetters,
  useExecutions,
  useFailureHeatmap,
  useHealth,
  useJobs,
  useRunners,
  useThroughput,
} from '~/api/queries'
import { formatDuration, formatRelative } from '~/lib/format'

/**
 * The landing page: is anything wrong, and what happens next.
 *
 * The decision from docs/ui-screen-inventory.md is a status board plus a
 * failures-only excerpt — deliberately *not* the shipping dashboard's general
 * "recent executions" list, which would be a fourth rendering of the runs
 * table. What replaces it is the upcoming rail, which answers a question
 * nothing in the product answered before.
 */
const { data: health } = useHealth()
const { data: jobs } = useJobs()
const { data: runners } = useRunners()
const { data: deadLetters } = useDeadLetters()
const { data: throughput } = useThroughput('24h')
const { data: heatmap } = useFailureHeatmap(7)

/** Only failures. A general run list belongs on /executions, once. */
const { data: failures } = useExecutions(() => ({ state: 'failed', limit: 5 }))

const activeJobs = computed(() => (jobs.value ?? []).filter((job) => job.is_active).length)
const online = computed(
  () => (runners.value ?? []).filter((runner) => runner.status === 'online').length,
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

const peak = computed(() => Math.max(1, ...buckets.value.map((b) => b.ok + b.err)))

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
    <div class="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
      <KpiCard
        label="Queue depth"
        :value="health?.queued ?? '—'"
        :sub="`${activeJobs} active job${activeJobs === 1 ? '' : 's'}`"
        :tone="(health?.queued ?? 0) > 0 ? 'warning' : 'default'"
        to="/executions?state=queued"
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
        to="/executions?state=failed"
      />
      <KpiCard
        label="Dead letters"
        :value="deadLetters?.length ?? 0"
        :sub="deadLetters?.length ? 'waiting for a decision' : 'none pending'"
        :tone="deadLetters?.length ? 'error' : 'success'"
        to="/dead-letters"
      />
    </div>

    <div class="grid gap-4 lg:grid-cols-[1fr_22rem]">
      <div class="flex flex-col gap-4">
        <!-- Throughput, ok over failed. Stacked rather than two lines: what
             matters is the proportion, and a second axis invites reading the
             error count as a trend of its own. -->
        <section class="rounded-xl border border-default bg-default p-4 shadow-sm">
          <div class="mb-3 flex items-baseline justify-between">
            <p class="cq-label">
              Throughput · last 24h
            </p>
            <span class="cq-num text-xs text-muted">{{ totals.ok + totals.err }} runs</span>
          </div>
          <AppEmpty
            v-if="buckets.length === 0"
            size="tight"
            icon="i-lucide-chart-column"
            title="No runs in the window"
          />
          <div
            v-else
            class="flex h-28 items-end gap-0.5"
            role="img"
            :aria-label="`${totals.ok} successful and ${totals.err} failed runs in the last 24 hours`"
          >
            <div
              v-for="bucket in buckets"
              :key="bucket.start"
              class="flex h-full min-w-0 flex-1 flex-col justify-end gap-px"
              :title="`${bucket.ok} ok · ${bucket.err} failed`"
            >
              <div
                v-if="bucket.err"
                class="rounded-t-sm bg-error"
                :style="{ height: `${(bucket.err / peak) * 100}%` }"
              />
              <div
                class="bg-success/70"
                :class="!bucket.err && 'rounded-t-sm'"
                :style="{ height: `${(bucket.ok / peak) * 100}%` }"
              />
            </div>
          </div>
        </section>

        <!-- Failures only. Not a general run list: that is /executions. -->
        <section class="rounded-xl border border-default bg-default p-4 shadow-sm">
          <div class="mb-3 flex items-baseline justify-between">
            <p class="cq-label">
              Recent failures
            </p>
            <RouterLink
              to="/executions?state=failed"
              class="text-xs text-primary hover:underline"
            >
              View all
            </RouterLink>
          </div>
          <AppEmpty
            v-if="!failures?.length"
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
              <span
                class="hidden min-w-0 max-w-[18rem] truncate text-xs text-muted md:block"
                :title="run.error ?? ''"
              >{{ run.error ?? '' }}</span>
              <span class="cq-num text-xs text-muted">{{ formatRelative(run.fire_at) }}</span>
              <span class="cq-num w-16 text-right text-xs text-muted">{{
                formatDuration(run.duration_ms)
              }}</span>
            </li>
          </ul>
        </section>
      </div>

      <div class="flex flex-col gap-4">
        <UpcomingRail />

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
                  class="h-3 min-w-0 flex-1 rounded-[2px]"
                  :class="count ? 'bg-error' : 'bg-elevated'"
                  :style="count ? { opacity: 0.35 + (count / heatMax) * 0.65 } : undefined"
                  :title="`${WEEKDAYS[dayIndex]} ${String(hour).padStart(2, '0')}:00 — ${count} failure${count === 1 ? '' : 's'}`"
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
