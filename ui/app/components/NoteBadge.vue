<script setup lang="ts">
import { computed } from 'vue'
import type { JobNote } from '~/api/types'
import { NOTE_KINDS, summarizeNotes } from '~/lib/notes'

/**
 * What colleagues have said about a run, at the size of a table cell.
 *
 * "Checked" leads and names who, because that is the question a failure list
 * is scanned for: has anyone looked at this yet. Open questions come next in
 * warning colour — they are the notes waiting on someone. Ideas and plain
 * notes collapse into a count. The full text is behind the hover.
 *
 * Renders nothing for a run without notes, so a list stays quiet until
 * someone has written something.
 */
const props = defineProps<{ notes?: readonly JobNote[] }>()

const summary = computed(() => summarizeNotes(props.notes ?? []))

const checkedLabel = computed(() => {
  const [first, ...rest] = summary.value.checkedBy
  return rest.length ? `${first} +${rest.length}` : (first ?? '')
})

const title = computed(() =>
  (props.notes ?? [])
    .map((note) => {
      const kind = NOTE_KINDS.find((entry) => entry.value === note.kind)?.label ?? note.kind
      return `${kind} · ${note.author_name}${note.body ? `: ${note.body}` : ''}`
    })
    .join('\n'),
)

const label = computed(() => {
  const parts: string[] = []
  if (summary.value.checkedBy.length) parts.push(`checked by ${summary.value.checkedBy.join(', ')}`)
  if (summary.value.questions) parts.push(`${summary.value.questions} open question(s)`)
  if (summary.value.others) parts.push(`${summary.value.others} note(s)`)
  return parts.join('; ')
})
</script>

<template>
  <span
    v-if="summary.total"
    v-tooltip="title"
    class="inline-flex shrink-0 items-center gap-1"
    :aria-label="label"
  >
    <UBadge
      v-if="summary.checkedBy.length"
      color="success"
      variant="subtle"
      size="sm"
      icon="i-lucide-check"
      class="max-w-[9rem]"
    >
      <span class="truncate">{{ checkedLabel }}</span>
    </UBadge>
    <UBadge
      v-if="summary.questions"
      color="warning"
      variant="subtle"
      size="sm"
      icon="i-lucide-circle-help"
    >
      {{ summary.questions }}
    </UBadge>
    <UBadge
      v-if="summary.others"
      color="neutral"
      variant="subtle"
      size="sm"
      icon="i-lucide-message-square"
    >
      {{ summary.others }}
    </UBadge>
  </span>
</template>
