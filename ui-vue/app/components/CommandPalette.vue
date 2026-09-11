<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useAlertsConfig, useCalendars, useJobs, useRunners } from '~/api/queries'
import { GO_TO } from '~/composables/useGoToShortcuts'

/**
 * Jump to anything, by name.
 *
 * The point of a palette in an operations console is that names are how people
 * hold the system in their heads — `demo:heartbeat`, `business-days`,
 * `demo-failures` — and clicking through a nav to find one is the slow path.
 *
 * Two differences from the React palette it replaces. It searches calendars
 * and alert rules too, which now have screens worth jumping to. And the
 * keyboard hints it prints are real: the React one advertised `G D`, `G J` and
 * the rest while nothing implemented them (see useGoToShortcuts).
 */
const open = defineModel<boolean>('open', { required: true })

const router = useRouter()
const query = ref('')
const cursor = ref(0)
const input = ref<HTMLInputElement | null>(null)

// Only fetched while the palette has been opened at least once — an operator
// who never uses it pays nothing for the four lists behind it.
const { data: jobs } = useJobs()
const { data: runners } = useRunners()
const { data: calendars } = useCalendars()
const { data: alerts } = useAlertsConfig()

interface Entry {
  id: string
  section: string
  icon: string
  label: string
  sub?: string
  hint?: string
  path: string
}

const needle = computed(() => query.value.trim().toLowerCase())

function matches(...fields: (string | null | undefined)[]): boolean {
  if (!needle.value) return true
  return fields.some((field) => (field ?? '').toLowerCase().includes(needle.value))
}

const entries = computed<Entry[]>(() => {
  const actions: Entry[] = GO_TO.filter((entry) => matches(entry.label)).map((entry) => ({
    id: `go-${entry.key}`,
    section: 'Go to',
    icon: 'i-lucide-corner-down-left',
    label: entry.label,
    hint: `g ${entry.key}`,
    path: entry.path,
  }))

  const jobEntries: Entry[] = (jobs.value ?? [])
    .filter((job) => matches(job.job_key, job.description))
    .slice(0, 6)
    .map((job) => ({
      id: `job-${job.job_key}`,
      section: 'Jobs',
      icon: 'i-lucide-briefcase',
      label: job.job_key,
      sub: job.description ?? undefined,
      path: `/jobs/${encodeURIComponent(job.job_key)}`,
    }))

  const runnerEntries: Entry[] = (runners.value ?? [])
    .filter((runner) => matches(runner.runner_id, runner.tags.join(' ')))
    .slice(0, 4)
    .map((runner) => ({
      id: `runner-${runner.runner_id}`,
      section: 'Runners',
      icon: 'i-lucide-cpu',
      label: runner.runner_id,
      sub: `${runner.status}${runner.tags.length ? ` · ${runner.tags.join(' ')}` : ''}`,
      // Runners have no detail route of their own by design — the list links
      // into the one run list instead, so that is where this goes.
      path: `/executions?runner_id=${encodeURIComponent(runner.runner_id)}`,
    }))

  const calendarEntries: Entry[] = (calendars.value ?? [])
    .filter((calendar) => matches(calendar.name))
    .slice(0, 4)
    .map((calendar) => ({
      id: `cal-${calendar.calendar_id}`,
      section: 'Calendars',
      icon: 'i-lucide-calendar-days',
      label: calendar.name,
      sub: calendar.timezone ?? 'UTC',
      path: `/calendars/${encodeURIComponent(calendar.calendar_id)}`,
    }))

  const ruleEntries: Entry[] = (alerts.value?.rules ?? [])
    .filter((rule) => matches(rule.name, rule.job_key_glob))
    .slice(0, 4)
    .map((rule) => ({
      id: `rule-${rule.name}`,
      section: 'Alert rules',
      icon: 'i-lucide-bell',
      label: rule.name,
      sub: rule.job_key_glob,
      path: `/alerts/rules/${encodeURIComponent(rule.name)}`,
    }))

  return [...actions, ...jobEntries, ...runnerEntries, ...calendarEntries, ...ruleEntries]
})

/** Section headings, computed from the list so an empty section never shows. */
function isFirstOfSection(index: number): boolean {
  return index === 0 || entries.value[index - 1]!.section !== entries.value[index]!.section
}

watch(entries, (next) => {
  if (cursor.value >= next.length) cursor.value = Math.max(0, next.length - 1)
})

watch(open, async (isOpen) => {
  if (!isOpen) return
  query.value = ''
  cursor.value = 0
  await nextTick()
  input.value?.focus()
})

function go(entry: Entry | undefined) {
  if (!entry) return
  open.value = false
  void router.push(entry.path)
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'ArrowDown') {
    cursor.value = Math.min(cursor.value + 1, entries.value.length - 1)
    event.preventDefault()
  } else if (event.key === 'ArrowUp') {
    cursor.value = Math.max(cursor.value - 1, 0)
    event.preventDefault()
  } else if (event.key === 'Enter') {
    event.preventDefault()
    go(entries.value[cursor.value])
  } else if (event.key === 'Escape') {
    open.value = false
  }
}
</script>

<template>
  <UModal
    v-model:open="open"
    :ui="{ content: 'max-w-xl' }"
    title="Command palette"
    description="Jump to a screen, a job, a runner, a calendar or an alert rule."
    :close="false"
  >
    <template #content>
      <!-- The title and description above are for the accessibility tree; the
           visible surface is the field and the list, which is what a palette
           is. -->
      <div
        class="flex max-h-[70vh] flex-col"
        @keydown="onKeydown"
      >
        <div class="flex items-center gap-2 border-b border-default px-4 py-3">
          <UIcon
            name="i-lucide-search"
            class="size-4 shrink-0 text-muted"
          />
          <input
            ref="input"
            v-model="query"
            class="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-dimmed"
            placeholder="Search jobs, runners, calendars, rules…"
            aria-label="Search everything"
            @input="cursor = 0"
          >
          <kbd class="cq-label rounded border border-default px-1.5 py-0.5">esc</kbd>
        </div>

        <div
          class="min-h-0 flex-1 overflow-y-auto p-2"
          role="listbox"
          aria-label="Results"
        >
          <AppEmpty
            v-if="entries.length === 0"
            size="tight"
            icon="i-lucide-search-x"
            title="Nothing matches"
            description="No screen, job, runner, calendar or alert rule by that name."
          />

          <template
            v-for="(entry, index) in entries"
            :key="entry.id"
          >
            <p
              v-if="isFirstOfSection(index)"
              class="cq-label px-2 pt-2 pb-1"
            >
              {{ entry.section }}
            </p>
            <button
              type="button"
              role="option"
              :aria-selected="index === cursor"
              class="flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left text-sm"
              :class="index === cursor ? 'bg-elevated text-highlighted' : 'hover:bg-elevated/60'"
              @click="go(entry)"
              @mousemove="cursor = index"
            >
              <UIcon
                :name="entry.icon"
                class="size-4 shrink-0 text-muted"
              />
              <span class="min-w-0 flex-1 truncate font-mono">{{ entry.label }}</span>
              <span
                v-if="entry.sub"
                class="hidden min-w-0 max-w-[16rem] truncate text-xs text-muted sm:block"
              >{{ entry.sub }}</span>
              <!-- Printed because it works. -->
              <kbd
                v-if="entry.hint"
                class="cq-label shrink-0 rounded border border-default px-1.5 py-0.5"
              >{{ entry.hint }}</kbd>
            </button>
          </template>
        </div>
      </div>
    </template>
  </UModal>
</template>
