<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ApiError } from '~/api/client'
import { useCreateCalendar, useUpdateCalendar } from '~/api/queries'
import type { CalendarDefinition } from '~/api/types'
import { parseCalendarRules, type CalendarRulePayload } from '~/lib/croniq-dsl'

/**
 * Create and edit a calendar.
 *
 * One difference from the React tree worth naming, because it is a capability
 * it did not have: editing an existing calendar opens in the **builder**, not
 * in the raw text box. The React version fell back to raw on edit with the
 * reasoning that parsing stored DSL back into the typed payload is
 * "best-effort" — but `parseCalendarRules` reports whether it succeeded, so
 * the honest version is to try, use the builder when the parse is clean, and
 * fall back to raw text when it is not. Editing is the common case; sending it
 * to the escape hatch by default made the builder a create-only feature.
 */
const props = defineProps<{ calendar?: CalendarDefinition }>()
const open = defineModel<boolean>('open', { required: true })
const emit = defineEmits<{ created: [id: string] }>()

const createCalendar = useCreateCalendar()
const updateCalendar = useUpdateCalendar()

const editing = computed(() => Boolean(props.calendar))

const name = ref('')
const timezone = ref('')
const rules = ref('')
const mode = ref<'builder' | 'raw'>('builder')
/** Seeds the builder when the stored DSL parses; `undefined` = its defaults. */
const initialRules = ref<CalendarRulePayload[] | undefined>(undefined)
/** Remounts the builder when the seed changes — it owns its rules after that. */
const builderKey = ref(0)

const error = ref<string | null>(null)
const ruleError = ref<string | null>(null)

watch(
  () => [open.value, props.calendar] as const,
  async ([isOpen, calendar]) => {
    if (!isOpen) return
    error.value = null
    ruleError.value = null
    name.value = calendar?.name ?? ''
    timezone.value = calendar?.timezone ?? ''
    rules.value = calendar?.rules ?? ''
    initialRules.value = undefined

    if (!calendar?.rules?.trim()) {
      mode.value = 'builder'
      builderKey.value += 1
      return
    }

    const parsed = await parseCalendarRules(calendar.rules)
    // A clean parse means the builder can represent what is stored. A dirty
    // one means the saved text says something the builder would quietly
    // change, so the saved text is what gets shown.
    if (parsed.ok && parsed.rules.length > 0) {
      initialRules.value = parsed.rules
      mode.value = 'builder'
    } else {
      mode.value = 'raw'
    }
    builderKey.value += 1
  },
  { immediate: true },
)

const pending = computed(
  () => createCalendar.isPending.value || updateCalendar.isPending.value,
)

async function submit() {
  error.value = null
  if (!name.value.trim()) {
    error.value = 'A calendar needs a name — it is what a schedule refers to.'
    return
  }
  try {
    if (props.calendar) {
      await updateCalendar.mutateAsync({
        calendar_id: props.calendar.calendar_id,
        name: name.value.trim(),
        // Empty clears the override, the same convention the schedule
        // endpoint uses.
        timezone: timezone.value.trim(),
        rules: rules.value,
      })
    } else {
      const created = await createCalendar.mutateAsync({
        name: name.value.trim(),
        timezone: timezone.value.trim() || undefined,
        rules: rules.value,
      })
      emit('created', created.calendar_id)
    }
    open.value = false
  } catch (caught) {
    const body = caught instanceof ApiError ? (caught.body as { message?: string }) : undefined
    // The server validates the rules too, and its parser is the authority —
    // its message beats anything this form could phrase.
    error.value = body?.message ?? (caught as Error).message ?? 'The server refused that.'
  }
}
</script>

<template>
  <UModal
    v-model:open="open"
    :title="editing ? `Edit ${calendar?.name}` : 'New calendar'"
    description="A calendar gates when a schedule may fire. Jobs refer to it by name."
  >
    <template #body>
      <div class="flex flex-col gap-4">
        <UAlert
          v-if="error"
          color="warning"
          variant="subtle"
          icon="i-lucide-shield-alert"
          :description="error"
          role="alert"
        />

        <div class="grid grid-cols-2 gap-4">
          <UFormField
            label="Name"
            description="What a schedule writes in its calendar field."
            required
          >
            <UInput
              v-model="name"
              placeholder="business-days"
              class="w-full font-mono"
              autofocus
            />
          </UFormField>
          <UFormField
            label="Timezone"
            description="The zone its rules are read in. Empty = UTC."
          >
            <UInput
              v-model="timezone"
              placeholder="Europe/Berlin"
              class="w-full font-mono"
            />
          </UFormField>
        </div>

        <div>
          <div class="mb-2 flex items-center justify-between gap-2">
            <p class="cq-label">
              Rules
            </p>
            <UButton
              variant="link"
              color="neutral"
              size="xs"
              class="p-0"
              @click="mode = mode === 'builder' ? 'raw' : 'builder'"
            >
              {{ mode === 'builder' ? 'Edit as text' : 'Back to the builder' }}
            </UButton>
          </div>

          <UAlert
            v-if="ruleError"
            class="mb-2"
            color="warning"
            variant="subtle"
            icon="i-lucide-triangle-alert"
            :description="ruleError"
            role="alert"
          />

          <CalendarRuleBuilder
            v-if="mode === 'builder'"
            :key="builderKey"
            :initial="initialRules"
            @change="(dsl: string) => (rules = dsl)"
            @error="(message: string | null) => (ruleError = message)"
          />
          <UTextarea
            v-else
            v-model="rules"
            :rows="8"
            class="w-full font-mono"
            aria-label="Calendar rules"
            placeholder="include weekday&#10;exclude 12-25"
          />
        </div>
      </div>
    </template>

    <template #footer>
      <div class="flex justify-end gap-2">
        <UButton
          color="neutral"
          variant="ghost"
          @click="open = false"
        >
          Cancel
        </UButton>
        <UButton
          :loading="pending"
          @click="submit"
        >
          {{ editing ? 'Save' : 'Create calendar' }}
        </UButton>
      </div>
    </template>
  </UModal>
</template>
