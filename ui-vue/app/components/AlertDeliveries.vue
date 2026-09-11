<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useAlertDeliveries } from '~/api/queries'
import type { AlertDelivery } from '~/api/types'
import { formatAbsolute, formatRelative, shortId } from '~/lib/format'

/**
 * What actually went out.
 *
 * The filters live in the URL, like the runs list — so "every failed delivery
 * for this rule" is a link you can paste into a ticket, which is most of what
 * this view is for.
 *
 * Each row links two ways, and that is its point: to the run that caused the
 * alert, and to the rule that decided to send it. In the React page the
 * delivery history was a flat table that named both and linked to neither.
 */
const route = useRoute()
const router = useRouter()

const STATES = ['delivered', 'failed', 'throttled']

const filters = computed(() => ({
  rule_name: (route.query.rule as string) || '',
  job_key: (route.query.job_key as string) || '',
  state: (route.query.state as string) || '',
}))

const { data, isPending, isError, error, refetch } = useAlertDeliveries(() => ({
  rule_name: filters.value.rule_name || undefined,
  job_key: filters.value.job_key || undefined,
  state: (filters.value.state || undefined) as AlertDelivery['state'] | undefined,
}))

const rows = computed<AlertDelivery[]>(() => data.value ?? [])

const hasFilters = computed(() =>
  Boolean(filters.value.rule_name || filters.value.job_key || filters.value.state),
)

function setFilter(key: 'rule' | 'job_key' | 'state', value: string) {
  const query = { ...route.query }
  if (value) query[key] = value
  else delete query[key]
  void router.replace({ path: '/alerts/deliveries', query })
}

/**
 * A throttled delivery is not a failure — the rule fired and the throttle
 * window swallowed it deliberately. Colouring it like an error would train
 * people to ignore the colour.
 */
function tone(state: AlertDelivery['state']) {
  switch (state) {
    case 'delivered':
      return { color: 'success' as const, dot: 'bg-success' }
    case 'failed':
      return { color: 'error' as const, dot: 'bg-error' }
    default:
      return { color: 'neutral' as const, dot: 'bg-dimmed' }
  }
}

/** How long the channel took to accept it. Only meaningful once delivered. */
function latency(delivery: AlertDelivery): string {
  if (!delivery.delivered_at) return '—'
  const ms = Date.parse(delivery.delivered_at) - Date.parse(delivery.fired_at)
  if (!Number.isFinite(ms) || ms < 0) return '—'
  return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`
}
</script>

<template>
  <div class="flex min-h-0 flex-col gap-3">
    <div class="flex flex-wrap items-center gap-2">
      <USelectMenu
        :model-value="filters.state || undefined"
        :items="STATES"
        placeholder="Any state"
        aria-label="Filter deliveries by state"
        class="w-40"
        @update:model-value="(value: string) => setFilter('state', value ?? '')"
      />
      <UInput
        :model-value="filters.rule_name"
        placeholder="Rule…"
        icon="i-lucide-bell"
        aria-label="Filter deliveries by rule"
        class="w-48"
        @update:model-value="(value: string) => setFilter('rule', value)"
      />
      <UInput
        :model-value="filters.job_key"
        placeholder="Job key…"
        icon="i-lucide-search"
        aria-label="Filter deliveries by job key"
        class="w-48"
        @update:model-value="(value: string) => setFilter('job_key', value)"
      />
      <UButton
        v-if="hasFilters"
        variant="ghost"
        color="neutral"
        icon="i-lucide-x"
        size="sm"
        @click="router.replace('/alerts/deliveries')"
      >
        Clear
      </UButton>
      <span class="cq-num ml-auto text-sm text-muted">{{ rows.length }} deliver{{ rows.length === 1 ? 'y' : 'ies' }}</span>
    </div>

    <div class="min-h-0 flex-1 overflow-auto rounded-lg border border-default">
      <AppLoading
        v-if="isPending"
        label="Loading deliveries"
      />
      <AppError
        v-else-if="isError"
        :error="error"
        :on-retry="() => refetch()"
      />
      <AppEmpty
        v-else-if="rows.length === 0"
        icon="i-lucide-bell"
        :title="hasFilters ? 'No deliveries match' : 'Nothing has been sent'"
        :description="
          hasFilters
            ? 'Nothing in the log matches these filters.'
            : 'A delivery is recorded whenever a rule fires. None has, which either means nothing has gone wrong or that no rule covers what did.'
        "
      >
        <template
          v-if="hasFilters"
          #action
        >
          <UButton
            variant="subtle"
            color="neutral"
            @click="router.replace('/alerts/deliveries')"
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
              Rule
            </th>
            <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
              Channel
            </th>
            <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
              Job
            </th>
            <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
              Run
            </th>
            <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-right">
              Fired
            </th>
            <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-right">
              Took
            </th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="delivery in rows"
            :key="delivery.delivery_id"
            class="cq-row border-b border-default/60"
          >
            <td class="px-[var(--cq-cell-x)]">
              <UBadge
                :color="tone(delivery.state).color"
                variant="subtle"
                size="sm"
                class="gap-1.5 whitespace-nowrap"
                :title="delivery.error ?? undefined"
              >
                <span
                  class="size-1.5 shrink-0 rounded-full"
                  :class="tone(delivery.state).dot"
                  aria-hidden="true"
                />
                {{ delivery.state }}
              </UBadge>
            </td>
            <td class="max-w-[12rem] truncate px-[var(--cq-cell-x)]">
              <RouterLink
                :to="`/alerts/rules/${encodeURIComponent(delivery.rule_name)}`"
                class="font-mono text-primary hover:underline"
              >
                {{ delivery.rule_name }}
              </RouterLink>
            </td>
            <td class="max-w-[10rem] truncate px-[var(--cq-cell-x)] font-mono text-muted">
              {{ delivery.channel_name }}
            </td>
            <td class="max-w-[12rem] truncate px-[var(--cq-cell-x)]">
              <RouterLink
                :to="`/jobs/${encodeURIComponent(delivery.job_key)}`"
                class="font-mono text-primary hover:underline"
              >
                {{ delivery.job_key }}
              </RouterLink>
            </td>
            <td class="cq-num px-[var(--cq-cell-x)] font-mono text-muted">
              <!-- Into the run that caused it. This is the link the React
                   page did not have, and the first thing anyone wants after
                   being paged. -->
              <RouterLink
                v-if="delivery.execution_id"
                :to="`/executions/${delivery.execution_id}`"
                class="text-primary hover:underline"
                :title="delivery.execution_id"
              >
                {{ shortId(delivery.execution_id) }}
              </RouterLink>
              <span v-else>—</span>
            </td>
            <td
              class="cq-num px-[var(--cq-cell-x)] text-right text-muted"
              :title="formatAbsolute(delivery.fired_at)"
            >
              {{ formatRelative(delivery.fired_at) }}
            </td>
            <td class="cq-num px-[var(--cq-cell-x)] text-right text-muted">
              {{ latency(delivery) }}
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>
