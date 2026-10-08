<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useCreateNote, useCurrentUser, useDeleteNote, useNotes } from '~/api/queries'
import type { JobNote, NoteKind } from '~/api/types'
import { useActionError } from '~/composables/useActionError'
import { useCanWriteNotes } from '~/composables/useCanWriteNotes'
import { formatAbsolute, formatRelative, shortId } from '~/lib/format'
import { NOTE_KINDS, noteIcon, noteTone } from '~/lib/notes'

/**
 * Notes colleagues left on a run or a job: "checked", questions, ideas.
 *
 * With `executionId` it shows that run's notes and writes new ones against
 * it; without, every note on the job, run-bound ones included, each linking
 * back to its run — or, with `jobOnly`, just the job's own: the notes inbox
 * shows a job's thread and its runs' threads as separate rows. A run that
 * retention has since deleted still has its notes here — the link then lands
 * on the Runs screen's "not in this list".
 *
 * `closed` keeps the thread readable and hides the composer: the server
 * refuses a new note on a run that no longer exists.
 *
 * Plain text, rendered with `whitespace-pre-wrap`. Markdown would mean a
 * dependency and HTML from user input in a page with a strict CSP (ADR-0005),
 * for notes that are a sentence or two.
 */
const props = defineProps<{
  jobKey: string
  executionId?: string | null
  jobOnly?: boolean
  closed?: boolean
}>()

const { data, isPending } = useNotes(() =>
  props.executionId
    ? { execution_ids: [props.executionId] }
    : // The job's own notes share the list with its runs' notes; ask for
      // enough that a busy job's runs do not crowd them out.
      { job_key: props.jobKey, limit: props.jobOnly ? 500 : undefined },
)
/**
 * Only the notes for what is on screen. `useNotes` keeps the previous answer
 * while a new one loads, which is right for a list of badges and wrong here:
 * switching runs would show the last run's notes for a moment.
 */
const notes = computed<JobNote[]>(() =>
  (data.value ?? []).filter((note) => {
    if (props.executionId) return note.execution_id === props.executionId
    if (props.jobOnly) return note.job_key === props.jobKey && note.execution_id === null
    return note.job_key === props.jobKey
  }),
)

const canWrite = useCanWriteNotes()
/** Write here: allowed to, and somewhere the server will take a note. */
const canCompose = computed(() => canWrite.value && !props.closed)
const { data: me } = useCurrentUser()
/** Presentation only — the server checks authorship on delete. */
function canDelete(note: JobNote): boolean {
  if (!me.value) return true
  return me.value.role === 'admin' || note.author_id === me.value.user_id
}

const create = useCreateNote()
const remove = useDeleteNote()
const { error, attempt } = useActionError()

const kind = ref<NoteKind>(props.executionId ? 'ack' : 'note')
const body = ref('')
// A new run or job starts a new note: text typed about the last one does not
// belong to this one.
watch(
  () => [props.jobKey, props.executionId],
  () => {
    body.value = ''
    kind.value = props.executionId ? 'ack' : 'note'
    error.value = null
  },
)

const current = computed(() => NOTE_KINDS.find((entry) => entry.value === kind.value)!)
const canSubmit = computed(() => kind.value === 'ack' || body.value.trim() !== '')

async function submit() {
  if (!canSubmit.value) return
  const ok = await attempt(() =>
    create.mutateAsync({
      job_key: props.jobKey,
      execution_id: props.executionId ?? null,
      kind: kind.value,
      body: body.value.trim(),
    }),
  )
  if (ok) body.value = ''
}

const pendingDelete = ref<JobNote | null>(null)
const deleteDescription = computed(() => {
  const note = pendingDelete.value
  if (!note) return undefined
  const label = NOTE_KINDS.find((entry) => entry.value === note.kind)?.label ?? 'note'
  return `The ${label.toLowerCase()} by ${note.author_name} will be gone for everyone who reads this ${props.executionId ? 'run' : 'job'}.`
})

async function confirmDelete() {
  const note = pendingDelete.value
  if (!note) return
  await attempt(() => remove.mutateAsync(note.id))
  pendingDelete.value = null
}
</script>

<template>
  <section aria-label="Notes">
    <AppLoading
      v-if="isPending && !notes.length"
      size="tight"
      label="Loading notes"
    />
    <p
      v-else-if="!notes.length"
      class="text-sm text-muted"
    >
      No notes yet.
      <template v-if="canCompose">
        Mark it as checked, or leave a question for whoever looks next.
      </template>
    </p>

    <ul
      v-else
      class="flex flex-col gap-3"
    >
      <li
        v-for="note in notes"
        :key="note.id"
        class="flex gap-2 text-sm"
      >
        <UIcon
          :name="noteIcon(note.kind)"
          class="mt-0.5 size-4 shrink-0"
          :class="noteTone(note.kind)"
          aria-hidden="true"
        />
        <div class="min-w-0 flex-1">
          <div class="flex items-baseline gap-2">
            <span class="truncate font-medium">{{ note.author_name }}</span>
            <span
              v-if="note.kind === 'ack'"
              class="text-xs text-success"
            >checked</span>
            <span
              v-else-if="note.kind === 'question'"
              class="text-xs text-warning"
            >asks</span>
            <span
              v-tooltip="formatAbsolute(note.created_at)"
              class="cq-num text-xs text-muted"
            >{{ formatRelative(note.created_at) }}</span>
            <RouterLink
              v-if="!executionId && note.execution_id"
              v-tooltip="note.execution_id"
              :to="`/executions/${note.execution_id}`"
              class="font-mono text-xs text-primary hover:underline"
            >
              run {{ shortId(note.execution_id) }}
            </RouterLink>
            <UButton
              v-if="canWrite && canDelete(note)"
              v-tooltip="'Delete this note'"
              icon="i-lucide-trash-2"
              color="neutral"
              variant="ghost"
              size="xs"
              class="ml-auto"
              aria-label="Delete this note"
              @click="pendingDelete = note"
            />
          </div>
          <p
            v-if="note.body"
            class="mt-0.5 break-words whitespace-pre-wrap"
          >
            {{ note.body }}
          </p>
        </div>
      </li>
    </ul>

    <form
      v-if="canCompose"
      class="mt-4 flex flex-col gap-2"
      @submit.prevent="submit"
    >
      <div
        class="flex flex-wrap gap-1"
        role="radiogroup"
        aria-label="Kind of note"
      >
        <UButton
          v-for="entry in NOTE_KINDS"
          :key="entry.value"
          :icon="entry.icon"
          size="xs"
          :color="kind === entry.value ? 'primary' : 'neutral'"
          :variant="kind === entry.value ? 'soft' : 'ghost'"
          role="radio"
          :aria-checked="kind === entry.value"
          @click="kind = entry.value"
        >
          {{ entry.label }}
        </UButton>
      </div>
      <UTextarea
        v-model="body"
        :rows="2"
        autoresize
        :maxlength="4000"
        class="w-full"
        :placeholder="current.placeholder"
        aria-label="Note text"
        @keydown.enter.ctrl.prevent="submit"
        @keydown.enter.meta.prevent="submit"
      />
      <div class="flex justify-end">
        <UButton
          type="submit"
          size="sm"
          :icon="current.icon"
          :disabled="!canSubmit"
          :loading="create.isPending.value"
        >
          {{ kind === 'ack' ? 'Mark as checked' : `Add ${current.label.toLowerCase()}` }}
        </UButton>
      </div>
    </form>

    <UAlert
      v-if="error"
      class="mt-3"
      color="error"
      variant="subtle"
      icon="i-lucide-alert-triangle"
      :description="error"
      role="alert"
      close
      @update:open="error = null"
    />

    <ConfirmModal
      :open="pendingDelete !== null"
      title="Delete this note?"
      :description="deleteDescription"
      :loading="remove.isPending.value"
      @update:open="(open: boolean) => { if (!open) pendingDelete = null }"
      @confirm="confirmDelete"
    />
  </section>
</template>
