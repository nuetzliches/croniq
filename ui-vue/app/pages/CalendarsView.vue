<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useCalendars, useJobStates, useSchedules } from '~/api/queries'
import type { CalendarDefinition } from '~/api/types'
import { formatRelative } from '~/lib/format'

/**
 * Calendars — the gates a schedule fires through.
 *
 * Same shape as every other list in the rebuild: a table, full width until
 * something is selected, detail beside it, addressable by URL.
 *
 * The column that is new is *Used by*. A calendar that nothing references is
 * not obviously broken — it just quietly gates nothing — and in the React
 * screen that state was indistinguishable from a working one. Counting the
 * schedules that name it makes an orphan visible from the list.
 */
const route = useRoute()
const router = useRouter()

const { data: calendars, isPending, isError, error, refetch } = useCalendars()
const { data: schedules } = useSchedules()
const { data: states } = useJobStates()

const selectedId = computed(() => (route.params.calendarId as string | undefined) ?? undefined)

/** How many schedules name each calendar, and how many it is holding now. */
const usage = computed(() => {
  const stateByKey = new Map((states.value ?? []).map((state) => [state.job_key, state]))
  const counts = new Map<string, { uses: number; held: number }>()
  for (const trigger of schedules.value ?? []) {
    if (!trigger.calendar) continue
    const entry = counts.get(trigger.calendar) ?? { uses: 0, held: 0 }
    entry.uses += 1
    if (stateByKey.get(trigger.job_key)?.suppressed_by?.includes(trigger.calendar)) entry.held += 1
    counts.set(trigger.calendar, entry)
  }
  return counts
})

const rows = computed(() =>
  [...(calendars.value ?? [])].sort((a, b) => a.name.localeCompare(b.name)),
)

const selected = computed(
  () => rows.value.find((calendar) => calendar.calendar_id === selectedId.value) ?? null,
)

function open(calendar: CalendarDefinition) {
  void router.push(`/calendars/${encodeURIComponent(calendar.calendar_id)}`)
}

function close() {
  void router.push('/calendars')
}

const creating = ref(false)

/** First line of the rules, as a preview; the rest is in the detail. */
function rulePreview(calendar: CalendarDefinition): string {
  const lines = calendar.rules
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
  if (lines.length === 0) return 'no rules — gates nothing'
  return lines.length === 1 ? lines[0]! : `${lines[0]} +${lines.length - 1}`
}

/** The soonest fire among the jobs this calendar gates, as a liveness hint. */
function nextThrough(calendar: CalendarDefinition): string {
  const gated = (schedules.value ?? []).filter((trigger) => trigger.calendar === calendar.name)
  if (gated.length === 0) return '—'
  const keys = new Set(gated.map((trigger) => trigger.job_key))
  const soonest = (states.value ?? [])
    .filter((state) => keys.has(state.job_key) && state.next_fire_at)
    .map((state) => Date.parse(state.next_fire_at!))
    .sort((a, b) => a - b)[0]
  return soonest ? formatRelative(new Date(soonest).toISOString()) : '—'
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col gap-4">
    <div class="flex flex-wrap items-center gap-2">
      <p class="text-sm text-muted">
        A calendar gates when a schedule may fire. Jobs refer to it by name.
      </p>
      <div class="ml-auto flex items-center gap-3">
        <span class="cq-num text-sm text-muted">{{ rows.length }} calendar{{ rows.length === 1 ? '' : 's' }}</span>
        <UButton
          icon="i-lucide-plus"
          size="sm"
          @click="creating = true"
        >
          New calendar
        </UButton>
      </div>
    </div>

    <div class="flex min-h-0 flex-1 gap-4">
      <div class="min-w-0 flex-1 overflow-auto rounded-lg border border-default">
        <AppLoading
          v-if="isPending"
          label="Loading calendars"
        />
        <AppError
          v-else-if="isError"
          :error="error"
          :on-retry="() => refetch()"
        />
        <AppEmpty
          v-else-if="rows.length === 0"
          icon="i-lucide-calendar-days"
          title="No calendars"
          description="Calendars come from the Croniqfile or from the API. Without one, a schedule fires whenever its rule says so."
        >
          <template #action>
            <UButton @click="creating = true">
              New calendar
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
                Calendar
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
                Rules
              </th>
              <th
                v-if="!selectedId"
                class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left"
              >
                Timezone
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-right">
                Used by
              </th>
              <th
                v-if="!selectedId"
                class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-right"
              >
                Next through it
              </th>
              <th
                v-if="!selectedId"
                class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left"
              >
                Source
              </th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="calendar in rows"
              :key="calendar.calendar_id"
              :class="[
                'cq-row cursor-pointer border-b border-default/60 transition-colors hover:bg-elevated',
                calendar.calendar_id === selectedId && 'bg-elevated',
              ]"
              @click="open(calendar)"
            >
              <td class="max-w-[14rem] truncate px-[var(--cq-cell-x)] font-mono text-primary">
                {{ calendar.name }}
              </td>
              <td class="max-w-[20rem] truncate px-[var(--cq-cell-x)] font-mono text-muted">
                {{ rulePreview(calendar) }}
              </td>
              <td
                v-if="!selectedId"
                class="px-[var(--cq-cell-x)] font-mono text-muted"
              >
                {{ calendar.timezone || 'UTC' }}
              </td>
              <!-- An unused calendar is not broken, it just does nothing —
                   and that was invisible before. -->
              <td class="cq-num px-[var(--cq-cell-x)] text-right">
                <span
                  v-if="usage.get(calendar.name)?.uses"
                  class="text-default"
                >
                  {{ usage.get(calendar.name)!.uses }}
                  <span
                    v-if="usage.get(calendar.name)!.held"
                    class="text-warning"
                    :title="`${usage.get(calendar.name)!.held} held by this gate right now`"
                  >· {{ usage.get(calendar.name)!.held }} held</span>
                </span>
                <span
                  v-else
                  class="text-muted"
                  title="No schedule names this calendar"
                >unused</span>
              </td>
              <td
                v-if="!selectedId"
                class="cq-num px-[var(--cq-cell-x)] text-right text-muted"
              >
                {{ nextThrough(calendar) }}
              </td>
              <td
                v-if="!selectedId"
                class="px-[var(--cq-cell-x)]"
              >
                <UBadge
                  :color="calendar.managed_by === 'dsl' ? 'neutral' : 'primary'"
                  variant="subtle"
                  size="sm"
                >
                  {{ calendar.managed_by === 'dsl' ? 'Croniqfile' : 'API' }}
                </UBadge>
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <CalendarDetail
        v-if="selectedId"
        :calendar="selected"
        :calendar-id="selectedId"
        class="w-[30rem] shrink-0"
        @close="close"
      />
    </div>

    <CalendarForm
      v-model:open="creating"
      @created="(id: string) => router.push(`/calendars/${encodeURIComponent(id)}`)"
    />
  </div>
</template>
