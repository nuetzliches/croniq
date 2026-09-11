<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useJobStates, useJobs, useSchedules } from '~/api/queries'
import type { JobDefinition, JobScheduleState, TriggerDefinition } from '~/api/types'
import { formatAbsolute, formatRelative } from '~/lib/format'

/**
 * Jobs — the configuration screen, and the one the audit hit hardest.
 *
 * Its predecessor had six tabs, two of which rendered the same executions
 * table with different row limits. The inventory cut it to two (overview
 * including the schedule, and the rendered DSL); executions link into
 * `/executions?job_key=…`, alerts to `/alerts`, audit to the settings audit
 * log. Nothing is lost, each thing has one place.
 *
 * The larger change is what the *list* shows. The React list was a column of
 * names, so "when does this fire next" and "is anything late" were questions
 * you answered by opening jobs one at a time. Three endpoints already had the
 * answer and were never joined here: `/v1/jobs` (the definition),
 * `/v1/jobs/states` (next fire, last fire, overdue, lifecycle) and
 * `/v1/schedules` (the cron rules). This screen joins them.
 */
const route = useRoute()
const router = useRouter()

const { data: jobs, isPending, isError, error, refetch } = useJobs()
const { data: states } = useJobStates()
// Every trigger at once rather than per job: the list needs a rule for each
// row, and one request beats one per row.
const { data: schedules } = useSchedules()

const search = ref((route.query.q as string) || '')
const tagFilter = computed(() => (route.query.tag as string) || '')

const selectedKey = computed(() => (route.params.jobKey as string | undefined) ?? undefined)

/** Keep the box in step with the URL when the query changes from elsewhere. */
watch(
  () => route.query.q,
  (next) => {
    const value = (next as string) || ''
    if (value !== search.value) search.value = value
  },
)

const stateByKey = computed(() => {
  const map = new Map<string, JobScheduleState>()
  for (const state of states.value ?? []) map.set(state.job_key, state)
  return map
})

const triggersByKey = computed(() => {
  const map = new Map<string, TriggerDefinition[]>()
  for (const trigger of schedules.value ?? []) {
    const list = map.get(trigger.job_key)
    if (list) list.push(trigger)
    else map.set(trigger.job_key, [trigger])
  }
  return map
})

/** Every tag in use, with a count — the same affordance the runners list has. */
const tags = computed(() => {
  const counts = new Map<string, number>()
  for (const job of jobs.value ?? []) {
    for (const tag of job.tags ?? []) counts.set(tag, (counts.get(tag) ?? 0) + 1)
  }
  return [...counts.entries()].sort(([a], [b]) => a.localeCompare(b))
})

interface Row {
  job: JobDefinition
  state: JobScheduleState | undefined
  triggers: TriggerDefinition[]
  dslManaged: boolean
}

const rows = computed<Row[]>(() => {
  const needle = search.value.trim().toLowerCase()
  return (jobs.value ?? [])
    .filter((job) => {
      if (tagFilter.value && !(job.tags ?? []).includes(tagFilter.value)) return false
      if (!needle) return true
      return (
        job.job_key.toLowerCase().includes(needle) ||
        (job.description ?? '').toLowerCase().includes(needle)
      )
    })
    .map((job) => {
      const triggers = triggersByKey.value.get(job.job_key) ?? []
      return {
        job,
        state: stateByKey.value.get(job.job_key),
        triggers,
        // A job whose triggers come from the Croniqfile is read-only here:
        // the next reload would overwrite anything written through the API.
        dslManaged: triggers.some((trigger) => trigger.managed_by === 'dsl'),
      }
    })
    .sort((a, b) => {
      // Late first, then by what fires soonest, then by name. An overdue job is
      // the most urgent thing this screen knows and sorting it to the top is
      // cheaper than asking the reader to scan for it.
      if (a.state?.overdue !== b.state?.overdue) return a.state?.overdue ? -1 : 1
      const at = a.state?.next_fire_at ? Date.parse(a.state.next_fire_at) : Infinity
      const bt = b.state?.next_fire_at ? Date.parse(b.state.next_fire_at) : Infinity
      if (at !== bt) return at - bt
      return a.job.job_key.localeCompare(b.job.job_key)
    })
})

const overdue = computed(() => rows.value.filter((row) => row.state?.overdue).length)

function setQuery(patch: Record<string, string | undefined>) {
  const query = { ...route.query }
  for (const [key, value] of Object.entries(patch)) {
    if (value) query[key] = value
    else delete query[key]
  }
  void router.replace({ path: selectedKey.value ? route.path : '/jobs', query })
}

function onSearch(value: string) {
  search.value = value
  setQuery({ q: value || undefined })
}

function open(row: Row) {
  void router.push({ path: `/jobs/${encodeURIComponent(row.job.job_key)}`, query: route.query })
}

function close() {
  void router.push({ path: '/jobs', query: route.query })
}

const selected = computed(() => rows.value.find((row) => row.job.job_key === selectedKey.value))
/**
 * A deep-linked job that the current filters hide still opens: the detail
 * resolves against the unfiltered list, so a link from an alert or a ticket
 * works regardless of what was typed in the box.
 */
const selectedJob = computed(
  () =>
    selected.value?.job ??
    (jobs.value ?? []).find((job) => job.job_key === selectedKey.value) ??
    null,
)

const creating = ref(false)

/**
 * The next fire, phrased for a column about the future.
 *
 * `formatRelative` says "just now" inside its five-second threshold, which
 * reads as the past — wrong here, where every value is something that has not
 * happened yet.
 */
function nextFire(row: Row): string {
  if (row.state?.overdue) return 'overdue'
  if (!row.state?.next_fire_at) return '—'
  const text = formatRelative(row.state.next_fire_at)
  return text === 'just now' ? 'due now' : text
}

/** Shown in the rule column; the rest of the triggers are in the detail. */
function ruleOf(row: Row): string {
  const first = row.triggers[0]
  if (!first) return '—'
  const extra = row.triggers.length > 1 ? ` +${row.triggers.length - 1}` : ''
  return `${first.cron_expression ?? '—'}${extra}`
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col gap-4">
    <div class="flex flex-wrap items-center gap-2">
      <UInput
        :model-value="search"
        placeholder="Job key or description…"
        icon="i-lucide-search"
        aria-label="Search jobs"
        class="w-64"
        @update:model-value="(value: string) => onSearch(value)"
      />
      <!--
        A menu rather than a row of chips. The runners list can afford chips
        because a fleet has a handful of tags; jobs are tagged per team, per
        environment and per kind, so the row would push the count and the
        primary action off the toolbar as soon as the deployment is real.
      -->
      <UDropdownMenu
        v-if="tags.length"
        :items="[
          tags.map(([tag, count]) => ({
            label: `${tag}  ${count}`,
            type: 'checkbox' as const,
            checked: tagFilter === tag,
            onSelect: () => setQuery({ tag: tagFilter === tag ? undefined : tag }),
          })),
        ]"
      >
        <UButton
          :variant="tagFilter ? 'subtle' : 'ghost'"
          color="neutral"
          size="sm"
          icon="i-lucide-tag"
          trailing-icon="i-lucide-chevron-down"
          :class="tagFilter && 'font-mono'"
        >
          {{ tagFilter || 'Tags' }}
        </UButton>
      </UDropdownMenu>
      <UButton
        v-if="tagFilter"
        icon="i-lucide-x"
        color="neutral"
        variant="ghost"
        size="sm"
        aria-label="Clear the tag filter"
        @click="setQuery({ tag: undefined })"
      />

      <div class="ml-auto flex items-center gap-3">
        <RouterLink
          v-if="overdue"
          to="/jobs"
          class="cq-num flex items-center gap-1.5 text-sm text-warning"
        >
          <UIcon
            name="i-lucide-clock-alert"
            class="size-4"
          />
          {{ overdue }} overdue
        </RouterLink>
        <span class="cq-num text-sm text-muted">{{ rows.length }} jobs</span>
        <UButton
          icon="i-lucide-plus"
          size="sm"
          @click="creating = true"
        >
          New job
        </UButton>
      </div>
    </div>

    <div class="flex min-h-0 flex-1 gap-4">
      <div class="min-w-0 flex-1 overflow-auto rounded-lg border border-default">
        <AppLoading
          v-if="isPending"
          label="Loading jobs"
        />
        <AppError
          v-else-if="isError"
          :error="error"
          :on-retry="() => refetch()"
        />
        <AppEmpty
          v-else-if="rows.length === 0"
          icon="i-lucide-briefcase"
          :title="search || tagFilter ? 'No jobs match' : 'No jobs yet'"
          :description="
            search || tagFilter
              ? 'Nothing here matches the search or the tag.'
              : 'Jobs come from the Croniqfile or from the API. Create one here, or declare it in the Croniqfile and reload.'
          "
        >
          <template #action>
            <UButton
              v-if="search || tagFilter"
              variant="subtle"
              color="neutral"
              @click="onSearch(''); setQuery({ tag: undefined })"
            >
              Clear
            </UButton>
            <UButton
              v-else
              @click="creating = true"
            >
              New job
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
                Status
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
                Job
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
                Rule
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-right">
                Next fire
              </th>
              <th
                v-if="!selectedKey"
                class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-right"
              >
                Last fire
              </th>
              <th
                v-if="!selectedKey"
                class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-right"
              >
                Fires
              </th>
              <th
                v-if="!selectedKey"
                class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left"
              >
                Source
              </th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="row in rows"
              :key="row.job.job_key"
              :class="[
                'cq-row cursor-pointer border-b border-default/60 transition-colors hover:bg-elevated',
                row.job.job_key === selectedKey && 'bg-elevated',
              ]"
              @click="open(row)"
            >
              <td class="px-[var(--cq-cell-x)]">
                <StatusPill :state="row.state?.status ?? (row.job.is_active ? 'active' : 'disabled')" />
              </td>
              <!-- The only two-line cell in the product, and the reason this
                   row is taller than `cq-row`'s minimum: a key over its
                   description is what makes the list readable to someone who
                   does not already know the keys by heart. -->
              <td class="max-w-[20rem] px-[var(--cq-cell-x)] py-1.5">
                <span class="block truncate font-mono text-primary">{{ row.job.job_key }}</span>
                <span
                  v-if="row.job.description"
                  class="block truncate text-xs text-muted"
                >{{ row.job.description }}</span>
              </td>
              <td class="max-w-[14rem] truncate px-[var(--cq-cell-x)] font-mono text-muted">
                {{ ruleOf(row) }}
              </td>
              <!-- The column the React list did not have, and the reason this
                   screen is worth opening less often. -->
              <td
                class="cq-num px-[var(--cq-cell-x)] text-right"
                :class="row.state?.overdue ? 'text-warning' : 'text-muted'"
                :title="formatAbsolute(row.state?.next_fire_at)"
              >
                {{ nextFire(row) }}
              </td>
              <td
                class="cq-num px-[var(--cq-cell-x)] text-right text-muted"
                :title="formatAbsolute(row.state?.last_fired_at)"
              >
                {{ formatRelative(row.state?.last_fired_at) }}
              </td>
              <td class="cq-num px-[var(--cq-cell-x)] text-right text-muted">
                {{ row.state?.fire_count ?? 0 }}
              </td>
              <td class="px-[var(--cq-cell-x)]">
                <UBadge
                  :color="row.dslManaged ? 'neutral' : 'primary'"
                  variant="subtle"
                  size="sm"
                  :title="
                    row.dslManaged
                      ? 'Declared in the Croniqfile — read-only here until adopted'
                      : 'Registered through the API — editable here'
                  "
                >
                  {{ row.dslManaged ? 'Croniqfile' : 'API' }}
                </UBadge>
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <JobDetail
        v-if="selectedKey"
        :job="selectedJob"
        :job-key="selectedKey"
        class="w-[30rem] shrink-0"
        @close="close"
      />
    </div>

    <JobForm
      v-model:open="creating"
      @created="(key: string) => router.push(`/jobs/${encodeURIComponent(key)}`)"
    />
  </div>
</template>
