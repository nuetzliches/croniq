<script setup lang="ts">
import { computed, ref, watchEffect } from 'vue'
import { useRouter } from 'vue-router'
import { ApiError } from '~/api/client'
import {
  useAdoptJob,
  useCalendars,
  useDeleteJob,
  useJobStates,
  useJobStats,
  useSchedules,
  useSetJobActive,
  useTriggerJob,
  useUnadoptJob,
} from '~/api/queries'
import type { JobDefinition } from '~/api/types'
import { formatAbsolute, formatDuration, formatRelative } from '~/lib/format'
import { renderJobDsl } from '~/lib/render-dsl'

/**
 * One job, beside the list.
 *
 * Six tabs became two. What went where:
 *
 * - *Overview* absorbed *Schedule*, because a job's rule is the first thing
 *   anyone opening a job wants and putting it one click away made the overview
 *   a page of fields with the important one missing.
 * - *Executions* is gone twice over: it rendered the same table as the runs
 *   list, once truncated and once not. The header links into
 *   `/executions?job_key=…` instead.
 * - *Alerts* and *Audit* were second renderings of surfaces that exist on
 *   their own screens.
 * - *DSL* stayed, because it is the only place in the product that shows a job
 *   in the form it is written in.
 */
const props = defineProps<{ job: JobDefinition | null; jobKey: string }>()
const emit = defineEmits<{ close: [] }>()

const router = useRouter()

const { data: schedules } = useSchedules(() => props.jobKey)
const { data: calendars } = useCalendars()
const { data: states } = useJobStates()
const { data: stats } = useJobStats(() => props.jobKey)

const trigger = useTriggerJob()
const setActive = useSetJobActive()
const removeJob = useDeleteJob()
const adopt = useAdoptJob()
const unadopt = useUnadoptJob()

const tab = ref<'overview' | 'dsl'>('overview')
const TABS = [
  { label: 'Overview', value: 'overview' },
  { label: 'DSL', value: 'dsl' },
]

/** What the last action said, when it said no. Mutations here get refused for
 *  real reasons — adoption needs a server policy, deletion needs the job not
 *  to be DSL-managed — and the server's own wording is the useful part. */
const actionError = ref<string | null>(null)
/** And what it said when it worked but did not do what you might assume. */
const actionNote = ref<string | null>(null)

const state = computed(() => (states.value ?? []).find((s) => s.job_key === props.jobKey))
const triggers = computed(() => schedules.value ?? [])
const dslManaged = computed(() => triggers.value.some((t) => t.managed_by === 'dsl'))

const editing = ref(false)
const confirmingDelete = ref(false)

/**
 * The rendered block, plus whatever the rendering could not carry across.
 *
 * Asynchronous because the formatter is the real one, from `croniq-config`
 * compiled to wasm — which is what makes the text parse. `watchEffect` rather
 * than a computed: a computed cannot await, and the version that could not
 * await is the version that hand-assembled invalid DSL.
 */
const dsl = ref('')
const dslNotes = ref<string[]>([])

watchEffect(async () => {
  if (!props.job) {
    dsl.value = ''
    dslNotes.value = []
    return
  }
  const rendered = await renderJobDsl(props.job, triggers.value, calendars.value)
  dsl.value = rendered.text
  dslNotes.value = rendered.notes
})

const copied = ref(false)

async function copyDsl() {
  try {
    await navigator.clipboard.writeText(dsl.value)
    copied.value = true
    setTimeout(() => (copied.value = false), 1500)
  } catch {
    // Clipboard access can be refused; the text is on screen and selectable.
  }
}

async function run<T>(fn: () => Promise<T>, note?: (result: T) => string | null) {
  actionError.value = null
  actionNote.value = null
  try {
    const result = await fn()
    if (note) actionNote.value = note(result)
  } catch (caught) {
    const body = caught instanceof ApiError ? (caught.body as { message?: string }) : undefined
    actionError.value = body?.message ?? (caught as Error).message ?? 'The server refused that.'
  }
}

function fireNow() {
  void run(
    () => trigger.mutateAsync(props.jobKey),
    // `deduplicated` means the server folded this into a run that was already
    // queued under the same idempotency key. Nothing new was started, and
    // saying "queued" would be a lie the run list quietly confirms.
    (result) =>
      result.deduplicated
        ? 'Coalesced into a run that was already queued for this job.'
        : `Queued as ${result.execution_id}.`,
  )
}

function toggleActive() {
  void run(() => setActive.mutateAsync({ jobKey: props.jobKey, active: !props.job?.is_active }))
}

async function doDelete() {
  await run(() => removeJob.mutateAsync(props.jobKey))
  if (!actionError.value) {
    confirmingDelete.value = false
    void router.push('/jobs')
  }
}

const facts = computed(() => {
  const job = props.job
  if (!job) return []
  return [
    { label: 'Description', value: job.description || '—' },
    { label: 'Tags', value: (job.tags ?? []).join(' ') || '—', mono: true },
    { label: 'Timeout', value: job.timeout ?? '5m (default)', mono: true },
    { label: 'Max retries', value: job.max_retries == null ? 'default' : String(job.max_retries) },
    // Only when it says something: `queued` is the norm, `ephemeral` is the
    // reason an empty execution history is expected rather than a fault.
    ...(state.value?.execution_mode === 'ephemeral'
      ? [{ label: 'Execution mode', value: 'ephemeral — no run history is kept' }]
      : []),
    ...(job.assigned_runner_id
      ? [{ label: 'Pinned runner', value: job.assigned_runner_id, mono: true }]
      : []),
    {
      label: 'Next fire',
      value: state.value?.next_fire_at
        ? `${formatAbsolute(state.value.next_fire_at)}${state.value.timezone ? ` (${state.value.timezone})` : ''}`
        : '—',
    },
    { label: 'Last fire', value: formatAbsolute(state.value?.last_fired_at) },
    { label: 'Fires', value: String(state.value?.fire_count ?? 0) },
    { label: 'Created', value: formatAbsolute(job.created_at) },
    { label: 'Updated', value: formatAbsolute(job.updated_at) },
  ]
})

/** The dead-letter policy, only where it departs from the system default. */
const deadLetterFacts = computed(() => {
  const job = props.job
  if (!job) return []
  return [
    ...(job.dead_letter_enabled === false
      ? [{ label: 'Dead letters', value: 'disabled — exhausted runs are dropped' }]
      : []),
    ...(job.dead_letter_retention
      ? [{ label: 'Retention', value: job.dead_letter_retention, mono: true }]
      : []),
    ...(job.dead_letter_replay_max_age
      ? [{ label: 'Replay max age', value: job.dead_letter_replay_max_age, mono: true }]
      : []),
    ...(job.dead_letter_operator_hint
      ? [{ label: 'Operator hint', value: job.dead_letter_operator_hint }]
      : []),
  ]
})
</script>

<template>
  <aside
    class="flex min-h-0 flex-col overflow-hidden rounded-lg border border-default"
    aria-label="Job detail"
  >
    <header class="flex shrink-0 items-center gap-2 border-b border-default px-4 py-3">
      <StatusPill
        v-if="job"
        :state="state?.status ?? (job.is_active ? 'active' : 'disabled')"
      />
      <span class="min-w-0 flex-1 truncate font-mono text-sm text-primary">{{ jobKey }}</span>
      <UButton
        icon="i-lucide-x"
        color="neutral"
        variant="ghost"
        size="xs"
        aria-label="Close job detail"
        @click="emit('close')"
      />
    </header>

    <div class="min-h-0 flex-1 overflow-y-auto">
      <AppEmpty
        v-if="!job"
        size="tight"
        icon="i-lucide-search-x"
        title="No such job"
        :description="`Nothing here is registered as ${jobKey}. It may have been deleted, or removed from the Croniqfile and reloaded.`"
      />

      <template v-else>
        <!-- Actions before facts: someone who opened a job usually came to do
             something to it, not to read it. -->
        <div class="flex flex-wrap items-center gap-1.5 border-b border-default px-4 py-3">
          <UButton
            icon="i-lucide-play"
            size="xs"
            :loading="trigger.isPending.value"
            @click="fireNow"
          >
            Run now
          </UButton>
          <UButton
            :icon="job.is_active ? 'i-lucide-pause' : 'i-lucide-play-circle'"
            color="neutral"
            variant="subtle"
            size="xs"
            :disabled="dslManaged"
            :title="dslManaged ? 'Declared in the Croniqfile — adopt it first' : undefined"
            :loading="setActive.isPending.value"
            @click="toggleActive"
          >
            {{ job.is_active ? 'Pause' : 'Resume' }}
          </UButton>
          <UButton
            icon="i-lucide-pencil"
            color="neutral"
            variant="subtle"
            size="xs"
            :disabled="dslManaged"
            :title="dslManaged ? 'Declared in the Croniqfile — adopt it first' : undefined"
            @click="editing = true"
          >
            Edit
          </UButton>
          <UButton
            :to="`/executions?job_key=${encodeURIComponent(jobKey)}`"
            icon="i-lucide-list"
            color="neutral"
            variant="ghost"
            size="xs"
          >
            Runs
          </UButton>

          <div class="ml-auto flex items-center gap-1.5">
            <!-- Adoption is the DSL/API boundary, and it is the one control
                 here whose meaning is not obvious from its label. -->
            <UButton
              v-if="dslManaged"
              icon="i-lucide-download"
              color="neutral"
              variant="subtle"
              size="xs"
              title="Copy this job and its schedule into the API store so they can be edited here. The Croniqfile definition is ignored until you release it again. Requires policy { dsl_adopt_on_mutate true }."
              :loading="adopt.isPending.value"
              @click="run(() => adopt.mutateAsync(jobKey))"
            >
              Adopt
            </UButton>
            <UButton
              v-else
              icon="i-lucide-undo-2"
              color="neutral"
              variant="ghost"
              size="xs"
              title="Drop the API copy so the next reload reinstates the Croniqfile definition. Has no effect on a job that was never in the Croniqfile."
              :loading="unadopt.isPending.value"
              @click="run(() => unadopt.mutateAsync(jobKey))"
            >
              Release
            </UButton>
            <UButton
              icon="i-lucide-trash-2"
              color="error"
              variant="ghost"
              size="xs"
              :disabled="dslManaged"
              :aria-label="`Delete ${jobKey}`"
              :title="dslManaged ? 'Declared in the Croniqfile — delete it there' : 'Delete'"
              @click="confirmingDelete = true"
            />
          </div>
        </div>

        <div class="p-4">
          <UAlert
            v-if="actionError"
            class="mb-4"
            color="warning"
            variant="subtle"
            icon="i-lucide-shield-alert"
            title="Refused"
            :description="actionError"
            role="alert"
            close
            @update:open="actionError = null"
          />
          <UAlert
            v-else-if="actionNote"
            class="mb-4"
            color="info"
            variant="subtle"
            icon="i-lucide-info"
            :description="actionNote"
            role="status"
            close
            @update:open="actionNote = null"
          />

          <!-- A fail-closed pause. It looks identical to a manual one in the
               status pill, and the difference is the whole story. -->
          <UAlert
            v-if="state?.config_error"
            class="mb-4"
            color="error"
            variant="subtle"
            icon="i-lucide-triangle-alert"
            title="Paused by a configuration error"
            :description="state.config_error"
          />
          <UAlert
            v-else-if="state?.suppressed_by"
            class="mb-4"
            color="neutral"
            variant="subtle"
            icon="i-lucide-clock"
            title="Waiting on a gate"
            :description="`Active and on time, but held outside its window by ${state.suppressed_by}.`"
          />

          <!-- `link`, not the default pill: a filled bar here reads louder
               than "Run now" above it, and the tabs are navigation, not the
               thing you came to do. -->
          <UTabs
            v-model="tab"
            :items="TABS"
            :content="false"
            variant="link"
            size="sm"
            class="mb-4"
          />

          <template v-if="tab === 'overview'">
            <!-- The numbers the old Executions tab was really being used for.
                 A success rate and three percentiles answer "is this job
                 healthy" without scrolling a table of runs. -->
            <div
              v-if="stats && stats.total > 0"
              class="mb-4 grid grid-cols-4 gap-px overflow-hidden rounded-lg border border-default bg-border"
            >
              <div
                v-for="cell in [
                  { label: 'Runs · 7d', value: String(stats.total) },
                  { label: 'Success', value: `${(stats.success_rate * 100).toFixed(1)}%` },
                  { label: 'p50', value: formatDuration(stats.p50_ms) },
                  { label: 'p95', value: formatDuration(stats.p95_ms) },
                ]"
                :key="cell.label"
                class="bg-default px-3 py-2"
              >
                <p class="cq-label">
                  {{ cell.label }}
                </p>
                <p class="cq-num text-sm">
                  {{ cell.value }}
                </p>
              </div>
            </div>
            <p
              v-if="stats && stats.last_failure_at"
              class="mb-4 text-xs text-muted"
            >
              Last failure {{ formatRelative(stats.last_failure_at) }} ·
              <RouterLink
                :to="`/executions?job_key=${encodeURIComponent(jobKey)}&state=failed`"
                class="text-primary hover:underline"
              >
                see the failures
              </RouterLink>
            </p>

            <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
              <template
                v-for="fact in facts"
                :key="fact.label"
              >
                <dt class="text-muted">
                  {{ fact.label }}
                </dt>
                <dd
                  class="min-w-0 truncate text-right"
                  :class="fact.mono && 'font-mono'"
                  :title="fact.value"
                >
                  {{ fact.value }}
                </dd>
              </template>
            </dl>

            <template v-if="deadLetterFacts.length">
              <p class="cq-label mt-5 mb-2">
                Dead-letter policy
              </p>
              <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
                <template
                  v-for="fact in deadLetterFacts"
                  :key="fact.label"
                >
                  <dt class="text-muted">
                    {{ fact.label }}
                  </dt>
                  <dd
                    class="min-w-0 truncate text-right"
                    :class="fact.mono && 'font-mono'"
                    :title="fact.value"
                  >
                    {{ fact.value }}
                  </dd>
                </template>
              </dl>
            </template>

            <ScheduleEditor
              class="mt-5"
              :job-key="jobKey"
              :triggers="triggers"
              :dsl-managed="dslManaged"
            />
          </template>

          <template v-else>
            <div class="mb-2 flex items-center justify-between gap-2">
              <p
                class="text-xs text-muted"
                title="There is no endpoint that returns a job's source text — an API-registered job never had any, so this is reconstructed rather than fetched. It is formatted by croniq's own compiler, so it parses."
              >
                Reconstructed from the live job and its first schedule.
              </p>
              <UButton
                v-if="dsl"
                :icon="copied ? 'i-lucide-check' : 'i-lucide-copy'"
                color="neutral"
                variant="ghost"
                size="xs"
                :aria-label="`Copy the DSL for ${jobKey}`"
                @click="copyDsl"
              />
            </div>

            <pre
              v-if="dsl"
              class="overflow-x-auto rounded-md border border-default bg-elevated p-3 font-mono text-xs"
            >{{ dsl }}</pre>

            <!-- What the API could not tell us, in full. A tab that silently
                 rendered less than the job does would be worse than one that
                 renders nothing. -->
            <UAlert
              v-for="(note, index) in dslNotes"
              :key="index"
              class="mt-2"
              :color="dsl ? 'neutral' : 'warning'"
              variant="subtle"
              :icon="dsl ? 'i-lucide-info' : 'i-lucide-triangle-alert'"
              :description="note"
            />

            <AppEmpty
              v-if="!dsl && dslNotes.length === 0"
              size="tight"
              icon="i-lucide-file-x"
              title="Nothing to render yet"
            />
          </template>
        </div>
      </template>
    </div>

    <JobForm
      v-if="job"
      v-model:open="editing"
      :job="job"
    />

    <UModal
      v-model:open="confirmingDelete"
      title="Delete this job?"
      :description="`${jobKey} and its schedules are removed from the API store. Runs already in the history stay.`"
    >
      <template #footer>
        <div class="flex justify-end gap-2">
          <UButton
            color="neutral"
            variant="ghost"
            @click="confirmingDelete = false"
          >
            Cancel
          </UButton>
          <UButton
            color="error"
            :loading="removeJob.isPending.value"
            @click="doDelete"
          >
            Delete
          </UButton>
        </div>
      </template>
    </UModal>
  </aside>
</template>
