<script setup lang="ts">
import { computed } from 'vue'
import { useCancelExecution, useExecutionLogs } from '~/api/queries'
import type { Execution } from '~/api/types'
import { formatAbsolute, formatDuration, formatRelative } from '~/lib/format'

/**
 * One run, beside the list rather than instead of it.
 *
 * `execution` can be null: the row is resolved out of the list the table
 * already holds, and a deep link to a run that has scrolled past the fetch
 * limit will not find one. Saying so is better than an empty panel.
 *
 * The definition-list layout is carried over from the React tree's detail
 * rails, which the audit rated as the most scannable thing in it — label left,
 * value right, monospace where the value is one.
 */
const props = defineProps<{ execution: Execution | null }>()
defineEmits<{ close: [] }>()

const { data: logs, isPending: logsPending } = useExecutionLogs(() => props.execution?.id)
const cancel = useCancelExecution()

/** Only a run that has not finished can be asked to stop. */
const cancellable = computed(() =>
  props.execution ? ['queued', 'claimed'].includes(props.execution.state) : false,
)

const facts = computed(() => {
  const run = props.execution
  if (!run) return []
  return [
    { label: 'Run', value: run.id, mono: true },
    { label: 'Job', value: run.job_key, mono: true },
    { label: 'Runner', value: run.runner_id ?? '—', mono: true },
    { label: 'Attempt', value: String(run.attempt) },
    { label: 'Fired', value: formatAbsolute(run.fire_at) },
    // scheduled_for is the logical trigger time, held constant across retries
    // and replay — it differs from fire_at exactly when this run is one, which
    // is when it is worth showing.
    ...(run.scheduled_for !== run.fire_at
      ? [{ label: 'Scheduled for', value: formatAbsolute(run.scheduled_for) }]
      : []),
    { label: 'Claimed', value: run.claimed_at ? formatAbsolute(run.claimed_at) : '—' },
    { label: 'Completed', value: run.completed_at ? formatAbsolute(run.completed_at) : '—' },
    { label: 'Duration', value: formatDuration(run.duration_ms) },
  ]
})
</script>

<template>
  <aside
    class="flex min-h-0 flex-col overflow-hidden rounded-lg border border-default"
    aria-label="Run detail"
  >
    <header class="flex shrink-0 items-center gap-2 border-b border-default px-4 py-3">
      <template v-if="execution">
        <StatusPill :state="execution.state" />
        <span
          class="min-w-0 flex-1 truncate font-mono text-sm text-muted"
          :title="execution.fire_at"
        >{{ formatRelative(execution.fire_at) }}</span>
      </template>
      <span
        v-else
        class="flex-1 text-sm text-muted"
      >Run</span>
      <UButton
        icon="i-lucide-x"
        color="neutral"
        variant="ghost"
        size="xs"
        aria-label="Close run detail"
        @click="$emit('close')"
      />
    </header>

    <div class="min-h-0 flex-1 overflow-y-auto p-4">
      <AppEmpty
        v-if="!execution"
        size="tight"
        icon="i-lucide-search-x"
        title="Run not in this list"
        description="It may be older than the rows loaded here. Clearing the filters, or widening them, will bring it back."
      />

      <template v-else>
        <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
          <template
            v-for="fact in facts"
            :key="fact.label"
          >
            <dt class="text-muted">
              {{ fact.label }}
            </dt>
            <dd
              class="truncate text-right"
              :class="fact.mono && 'font-mono'"
              :title="fact.value"
            >
              {{ fact.value }}
            </dd>
          </template>
        </dl>

        <!-- The error, in full and selectable. Truncating the one field that
             says why something broke is the wrong economy. -->
        <div
          v-if="execution.error"
          class="mt-4"
        >
          <p class="cq-label mb-1.5 text-error">
            Error
          </p>
          <pre class="overflow-x-auto rounded-md border border-default bg-elevated p-3 font-mono text-xs whitespace-pre-wrap">{{ execution.error }}</pre>
        </div>

        <div class="mt-4">
          <p class="cq-label mb-1.5">
            Logs
          </p>
          <AppLoading
            v-if="logsPending"
            size="tight"
            label="Loading logs"
          />
          <AppEmpty
            v-else-if="!logs?.length"
            size="tight"
            icon="i-lucide-file-text"
            title="No log events"
            description="This run pushed none. Runners send them through the work events API."
          />
          <!-- A terminal panel, the way the console screen does it: log output
               reads as log output. -->
          <div
            v-else
            class="overflow-x-auto rounded-md bg-inverted p-3 font-mono text-xs"
          >
            <div
              v-for="entry in logs"
              :key="entry.id"
              class="flex gap-2 whitespace-pre-wrap"
            >
              <span class="shrink-0 text-dimmed">{{ entry.timestamp.slice(11, 19) }}</span>
              <span
                class="w-10 shrink-0 uppercase"
                :class="{
                  'text-error': entry.level === 'error',
                  'text-warning': entry.level === 'warn',
                  'text-inverted/70': entry.level !== 'error' && entry.level !== 'warn',
                }"
              >{{ entry.level }}</span>
              <span class="min-w-0 text-inverted">{{ entry.message }}</span>
            </div>
          </div>
        </div>

        <UButton
          v-if="cancellable"
          color="error"
          variant="subtle"
          icon="i-lucide-square"
          class="mt-4"
          :loading="cancel.isPending.value"
          @click="cancel.mutate(execution.id)"
        >
          Cancel run
        </UButton>
      </template>
    </div>
  </aside>
</template>
