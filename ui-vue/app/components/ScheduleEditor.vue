<script setup lang="ts">
import { computed, ref } from 'vue'
import { ApiError } from '~/api/client'
import {
  useCalendars,
  useCreateSchedule,
  useDeleteSchedule,
  useUpdateSchedule,
} from '~/api/queries'
import type { TriggerDefinition } from '~/api/types'

/**
 * A job's triggers, inside the overview rather than behind a tab of their own.
 *
 * The React tree gave the schedule its own tab, which meant the answer to
 * "when does this run" was one click from the page whose entire subject is a
 * job. It is four fields; it fits here.
 *
 * A DSL-managed job is read-only: the next Croniqfile reload would overwrite
 * anything written through the API, so the form is not offered rather than
 * offered and rejected.
 */
const props = defineProps<{
  jobKey: string
  triggers: TriggerDefinition[]
  dslManaged: boolean
}>()

const { data: calendars } = useCalendars()
const createSchedule = useCreateSchedule()
const updateSchedule = useUpdateSchedule()
const deleteSchedule = useDeleteSchedule()

const error = ref<string | null>(null)
/** The trigger being edited, or `'new'`, or nothing. */
const editing = ref<string | null>(null)

const blank = () => ({ cron_expression: '', timezone: '', calendar: '', window: '' })
const form = ref(blank())

const calendarNames = computed(() => (calendars.value ?? []).map((calendar) => calendar.name))

function startNew() {
  form.value = blank()
  error.value = null
  editing.value = 'new'
}

function startEdit(trigger: TriggerDefinition) {
  form.value = {
    cron_expression: trigger.cron_expression ?? '',
    timezone: trigger.timezone ?? '',
    calendar: trigger.calendar ?? '',
    window: trigger.window ?? '',
  }
  error.value = null
  editing.value = trigger.trigger_id
}

const orNull = (value: string) => (value.trim() ? value.trim() : null)

async function attempt(fn: () => Promise<unknown>) {
  error.value = null
  try {
    await fn()
    editing.value = null
  } catch (caught) {
    const body = caught instanceof ApiError ? (caught.body as { message?: string }) : undefined
    // A bad cron expression is refused here, and the server's parser message
    // is far more useful than anything this form could guess at.
    error.value = body?.message ?? (caught as Error).message ?? 'The server refused that.'
  }
}

function save() {
  const rule = form.value.cron_expression.trim()
  if (!rule) {
    error.value = 'A schedule needs a rule.'
    return
  }
  const patch = {
    cron_expression: rule,
    timezone: orNull(form.value.timezone),
    calendar: orNull(form.value.calendar),
    window: orNull(form.value.window),
  }
  void attempt(() =>
    editing.value === 'new'
      ? createSchedule.mutateAsync({ job_key: props.jobKey, ...patch })
      : updateSchedule.mutateAsync({ trigger_id: editing.value!, ...patch }),
  )
}

function toggleEnabled(trigger: TriggerDefinition) {
  void attempt(() =>
    updateSchedule.mutateAsync({ trigger_id: trigger.trigger_id, enabled: !trigger.enabled }),
  )
}

function remove(trigger: TriggerDefinition) {
  void attempt(() => deleteSchedule.mutateAsync(trigger.trigger_id))
}

const pending = computed(
  () =>
    createSchedule.isPending.value ||
    updateSchedule.isPending.value ||
    deleteSchedule.isPending.value,
)
</script>

<template>
  <section>
    <div class="mb-2 flex items-center justify-between gap-2">
      <p class="cq-label">
        Schedule
      </p>
      <UButton
        v-if="!dslManaged && editing !== 'new'"
        icon="i-lucide-plus"
        color="neutral"
        variant="ghost"
        size="xs"
        @click="startNew"
      >
        Add
      </UButton>
    </div>

    <UAlert
      v-if="error"
      class="mb-3"
      color="warning"
      variant="subtle"
      icon="i-lucide-shield-alert"
      :description="error"
      role="alert"
      close
      @update:open="error = null"
    />

    <p
      v-if="dslManaged"
      class="mb-3 text-xs text-muted"
    >
      Declared in the Croniqfile. Adopt the job to edit its schedule here.
    </p>

    <AppEmpty
      v-if="triggers.length === 0 && editing !== 'new'"
      size="tight"
      icon="i-lucide-calendar-off"
      title="No schedule"
      description="This job only runs when something triggers it — by hand, or through the API."
    />

    <ul class="flex flex-col gap-2">
      <li
        v-for="trigger in triggers"
        :key="trigger.trigger_id"
        class="rounded-lg border border-default p-3"
        :class="!trigger.enabled && 'opacity-60'"
      >
        <template v-if="editing === trigger.trigger_id">
          <div class="flex flex-col gap-3">
            <UFormField
              label="Rule"
              description="Cron, or one of the DSL shorthands."
            >
              <UInput
                v-model="form.cron_expression"
                class="w-full font-mono"
                placeholder="0 3 * * *"
              />
            </UFormField>
            <div class="grid grid-cols-2 gap-3">
              <UFormField
                label="Timezone"
                description="Empty inherits the server default."
              >
                <UInput
                  v-model="form.timezone"
                  class="w-full font-mono"
                  placeholder="Europe/Berlin"
                />
              </UFormField>
              <UFormField
                label="Window"
                description="Grace period for a late fire."
              >
                <UInput
                  v-model="form.window"
                  class="w-full font-mono"
                  placeholder="30m"
                />
              </UFormField>
            </div>
            <UFormField
              label="Calendar"
              description="Restricts firing to the days this calendar allows."
            >
              <USelectMenu
                v-model="form.calendar"
                :items="calendarNames"
                class="w-full"
                placeholder="None"
              />
            </UFormField>
            <div class="flex justify-end gap-2">
              <UButton
                color="neutral"
                variant="ghost"
                size="xs"
                @click="editing = null"
              >
                Cancel
              </UButton>
              <UButton
                size="xs"
                :loading="pending"
                @click="save"
              >
                Save
              </UButton>
            </div>
          </div>
        </template>

        <template v-else>
          <div class="flex items-start gap-2">
            <div class="min-w-0 flex-1">
              <p class="truncate font-mono text-sm">
                {{ trigger.cron_expression ?? '—' }}
              </p>
              <p class="mt-0.5 flex flex-wrap gap-x-3 text-xs text-muted">
                <span v-if="trigger.timezone">{{ trigger.timezone }}</span>
                <span v-if="trigger.calendar">calendar {{ trigger.calendar }}</span>
                <span v-if="trigger.window">window {{ trigger.window }}</span>
                <span v-if="!trigger.enabled">disabled</span>
              </p>
            </div>
            <div
              v-if="!dslManaged"
              class="flex shrink-0 items-center gap-1"
            >
              <UButton
                :icon="trigger.enabled ? 'i-lucide-pause' : 'i-lucide-play'"
                color="neutral"
                variant="ghost"
                size="xs"
                :aria-label="trigger.enabled ? 'Disable this schedule' : 'Enable this schedule'"
                :loading="pending"
                @click="toggleEnabled(trigger)"
              />
              <UButton
                icon="i-lucide-pencil"
                color="neutral"
                variant="ghost"
                size="xs"
                aria-label="Edit this schedule"
                @click="startEdit(trigger)"
              />
              <UButton
                icon="i-lucide-trash-2"
                color="error"
                variant="ghost"
                size="xs"
                aria-label="Delete this schedule"
                :loading="pending"
                @click="remove(trigger)"
              />
            </div>
          </div>
        </template>
      </li>

      <li
        v-if="editing === 'new'"
        class="rounded-lg border border-dashed border-default p-3"
      >
        <div class="flex flex-col gap-3">
          <UFormField
            label="Rule"
            description="Cron, or one of the DSL shorthands."
          >
            <UInput
              v-model="form.cron_expression"
              class="w-full font-mono"
              placeholder="0 3 * * *"
              autofocus
            />
          </UFormField>
          <div class="grid grid-cols-2 gap-3">
            <UFormField label="Timezone">
              <UInput
                v-model="form.timezone"
                class="w-full font-mono"
                placeholder="Europe/Berlin"
              />
            </UFormField>
            <UFormField label="Window">
              <UInput
                v-model="form.window"
                class="w-full font-mono"
                placeholder="30m"
              />
            </UFormField>
          </div>
          <UFormField label="Calendar">
            <USelectMenu
              v-model="form.calendar"
              :items="calendarNames"
              class="w-full"
              placeholder="None"
            />
          </UFormField>
          <div class="flex justify-end gap-2">
            <UButton
              color="neutral"
              variant="ghost"
              size="xs"
              @click="editing = null"
            >
              Cancel
            </UButton>
            <UButton
              size="xs"
              :loading="pending"
              @click="save"
            >
              Add schedule
            </UButton>
          </div>
        </div>
      </li>
    </ul>
  </section>
</template>
