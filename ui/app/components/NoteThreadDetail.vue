<script setup lang="ts">
import { computed } from 'vue'
import type { NoteThread } from '~/api/types'
import NotesPanel from '~/components/NotesPanel.vue'
import { shortId } from '~/lib/format'
import type { ThreadSelection } from '~/lib/notes-inbox'

/**
 * A thread of the notes inbox with no run to show beside it: a job's own
 * notes, or a run that retention has since deleted. A run that still exists
 * opens in `RunDetail` instead, its notes included.
 *
 * A job's thread renders from the route alone — its notes are asked for by
 * job key, so a link to one works even when the list does not hold the row.
 * A run's needs the row to know whether the run still exists; without it the
 * panel says the thread is not in this list, and offers the way back.
 */
const props = defineProps<{
  thread: NoteThread | null
  selection: ThreadSelection
  /** The list is narrowed, so showing everything might bring the thread back. */
  filtered: boolean
}>()
defineEmits<{ close: []; 'show-all': [] }>()

const jobKey = computed(() =>
  props.selection.kind === 'job' ? props.selection.jobKey : (props.thread?.job_key ?? ''),
)
const runId = computed(() => (props.selection.kind === 'run' ? props.selection.id : null))
const missing = computed(() => props.selection.kind === 'run' && !props.thread)
</script>

<template>
  <aside
    class="flex min-h-0 flex-col overflow-hidden rounded-lg border border-default"
    aria-label="Thread detail"
  >
    <header class="flex shrink-0 items-center gap-2 border-b border-default px-4 py-3">
      <UBadge
        v-if="selection.kind === 'job'"
        color="neutral"
        variant="outline"
        size="sm"
        icon="i-lucide-briefcase"
      >
        job
      </UBadge>
      <UBadge
        v-else-if="!missing"
        color="neutral"
        variant="outline"
        size="sm"
        icon="i-lucide-archive"
      >
        run deleted
      </UBadge>
      <span
        v-tooltip="runId ?? jobKey"
        class="min-w-0 flex-1 truncate font-mono text-sm text-muted"
      >{{ runId ? `run ${shortId(runId)}` : jobKey }}</span>
      <UButton
        icon="i-lucide-x"
        color="neutral"
        variant="ghost"
        size="xs"
        aria-label="Close thread"
        @click="$emit('close')"
      />
    </header>

    <div class="min-h-0 flex-1 overflow-y-auto p-4">
      <AppEmpty
        v-if="missing"
        size="tight"
        icon="i-lucide-search-x"
        title="Thread not in this list"
        :description="
          filtered
            ? 'The filter leaves it out. Showing every thread brings it back.'
            : 'It may be older than the threads loaded here. The run may still be on the Runs screen.'
        "
      >
        <template #action>
          <div class="flex flex-wrap justify-center gap-2">
            <UButton
              v-if="filtered"
              variant="subtle"
              color="neutral"
              @click="$emit('show-all')"
            >
              Show all
            </UButton>
            <UButton
              variant="ghost"
              color="neutral"
              icon="i-lucide-external-link"
              :to="`/executions/${runId}`"
            >
              Open in Runs
            </UButton>
          </div>
        </template>
      </AppEmpty>

      <template v-else>
        <p
          v-if="runId"
          class="mb-4 text-sm text-muted"
        >
          Retention has deleted this run and its log. Its notes stay: what was found out about it is
          still worth reading.
        </p>
        <dl class="mb-4 grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
          <dt class="text-muted">
            Job
          </dt>
          <dd
            v-tooltip="jobKey"
            class="min-w-0 truncate text-right font-mono"
          >
            <RouterLink
              :to="`/jobs/${encodeURIComponent(jobKey)}`"
              class="text-primary hover:underline"
            >
              {{ jobKey }}
            </RouterLink>
          </dd>
          <template v-if="runId">
            <dt class="text-muted">
              Run
            </dt>
            <dd
              v-tooltip="runId"
              class="min-w-0 truncate text-right font-mono"
            >
              {{ runId }}
            </dd>
          </template>
        </dl>
        <p class="cq-label mb-1.5">
          Notes
        </p>
        <!-- A deleted run's thread is read-only: the server refuses a note on
             a run that no longer exists, so offering to write one would only
             lead to that refusal. -->
        <NotesPanel
          :job-key="jobKey"
          :execution-id="runId"
          :job-only="selection.kind === 'job'"
          :closed="selection.kind === 'run'"
        />
      </template>
    </div>
  </aside>
</template>
