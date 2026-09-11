<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import { ApiError } from '~/api/client'
import {
  useAdoptCalendar,
  useDeleteCalendar,
  useJobStates,
  useSchedules,
  useUnadoptCalendar,
} from '~/api/queries'
import type { CalendarDefinition } from '~/api/types'
import { formatAbsolute, formatRelative } from '~/lib/format'

/**
 * One calendar, and — the part that is new — what it is actually doing.
 *
 * The React screen showed a calendar's name, timezone and rule text and
 * nothing else, which means the one question you open a calendar to answer
 * ("is this gate doing what I think?") had no answer anywhere in the product.
 * You wrote rules and found out later, from a job that did or did not fire.
 *
 * There is no endpoint that evaluates a calendar over a date range, and
 * nothing on the client may guess at the semantics — the DSL belongs to the
 * Rust side and a second implementation of it would be wrong eventually.
 *
 * But the server already applies the gate, and it already says so. Joining
 * `/v1/schedules` (which schedule names which calendar) with `/v1/jobs/states`
 * (whose `next_fire_at` is computed *through* the gate, and whose
 * `suppressed_by` names the gate when it is currently holding a job) answers
 * the question empirically: here are the jobs this calendar governs, here is
 * when each of them fires next, and here is which ones it is holding right
 * now.
 */
const props = defineProps<{ calendar: CalendarDefinition | null; calendarId: string }>()
const emit = defineEmits<{ close: [] }>()

const router = useRouter()
const { data: schedules } = useSchedules()
const { data: states } = useJobStates()

const removeCalendar = useDeleteCalendar()
const adopt = useAdoptCalendar()
const unadopt = useUnadoptCalendar()

const editing = ref(false)
const confirmingDelete = ref(false)
const actionError = ref<string | null>(null)

const dslManaged = computed(() => props.calendar?.managed_by === 'dsl')

/** Jobs whose schedule names this calendar, with their live scheduling state. */
const governed = computed(() => {
  const name = props.calendar?.name
  if (!name) return []
  const stateByKey = new Map((states.value ?? []).map((state) => [state.job_key, state]))
  return (schedules.value ?? [])
    .filter((trigger) => trigger.calendar === name)
    .map((trigger) => ({
      trigger,
      state: stateByKey.get(trigger.job_key),
    }))
    .sort((a, b) => a.trigger.job_key.localeCompare(b.trigger.job_key))
})

/**
 * Jobs this calendar is holding at this instant. `suppressed_by` names the
 * blocking gate, e.g. `calendar 'business-days'` — the substring test is
 * deliberately loose because the wording is the server's, not a contract.
 */
const held = computed(() =>
  governed.value.filter((row) => row.state?.suppressed_by?.includes(props.calendar?.name ?? '\0')),
)

async function run(fn: () => Promise<unknown>) {
  actionError.value = null
  try {
    await fn()
  } catch (caught) {
    const body = caught instanceof ApiError ? (caught.body as { message?: string }) : undefined
    actionError.value = body?.message ?? (caught as Error).message ?? 'The server refused that.'
  }
}

async function doDelete() {
  await run(() => removeCalendar.mutateAsync(props.calendarId))
  if (!actionError.value) {
    confirmingDelete.value = false
    void router.push('/calendars')
  }
}

const ruleLines = computed(() =>
  (props.calendar?.rules ?? '')
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean),
)
</script>

<template>
  <aside
    class="flex min-h-0 flex-col overflow-hidden rounded-lg border border-default"
    aria-label="Calendar detail"
  >
    <header class="flex shrink-0 items-center gap-2 border-b border-default px-4 py-3">
      <span class="min-w-0 flex-1 truncate font-mono text-sm text-primary">{{
        calendar?.name ?? calendarId
      }}</span>
      <UBadge
        v-if="calendar"
        :color="dslManaged ? 'neutral' : 'primary'"
        variant="subtle"
        size="sm"
      >
        {{ dslManaged ? 'Croniqfile' : 'API' }}
      </UBadge>
      <UButton
        icon="i-lucide-x"
        color="neutral"
        variant="ghost"
        size="xs"
        aria-label="Close calendar detail"
        @click="emit('close')"
      />
    </header>

    <div class="min-h-0 flex-1 overflow-y-auto">
      <AppEmpty
        v-if="!calendar"
        size="tight"
        icon="i-lucide-search-x"
        title="No such calendar"
        description="It may have been deleted, or removed from the Croniqfile and reloaded."
      />

      <template v-else>
        <div class="flex flex-wrap items-center gap-1.5 border-b border-default px-4 py-3">
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
          <div class="ml-auto flex items-center gap-1.5">
            <UButton
              v-if="dslManaged"
              icon="i-lucide-download"
              color="neutral"
              variant="subtle"
              size="xs"
              title="Copy this calendar into the API store so it can be edited. The Croniqfile definition is ignored until you release it. Requires policy { dsl_adopt_on_mutate true }."
              :loading="adopt.isPending.value"
              @click="run(() => adopt.mutateAsync(calendarId))"
            >
              Adopt
            </UButton>
            <UButton
              v-else
              icon="i-lucide-undo-2"
              color="neutral"
              variant="ghost"
              size="xs"
              title="Drop the API copy so the next reload reinstates the Croniqfile definition."
              :loading="unadopt.isPending.value"
              @click="run(() => unadopt.mutateAsync(calendarId))"
            >
              Release
            </UButton>
            <UButton
              icon="i-lucide-trash-2"
              color="error"
              variant="ghost"
              size="xs"
              :disabled="dslManaged"
              :aria-label="`Delete ${calendar.name}`"
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

          <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
            <dt class="text-muted">
              Timezone
            </dt>
            <dd class="text-right font-mono">
              {{ calendar.timezone || 'UTC (default)' }}
            </dd>
            <dt class="text-muted">
              Updated
            </dt>
            <dd class="text-right">
              {{ formatAbsolute(calendar.updated_at) }}
            </dd>
          </dl>

          <p class="cq-label mt-5 mb-2">
            Rules
          </p>
          <AppEmpty
            v-if="ruleLines.length === 0"
            size="tight"
            icon="i-lucide-calendar-off"
            title="No rules"
            description="A calendar with no rules allows everything, so a schedule bound to it is ungated."
          />
          <ul
            v-else
            class="flex flex-col gap-1"
          >
            <!-- Each rule on its own line with its verb marked. The React
                 screen showed the whole thing as one block of text, which
                 reads fine for two rules and not at all for ten. -->
            <li
              v-for="(line, index) in ruleLines"
              :key="index"
              class="flex items-baseline gap-2 font-mono text-xs"
            >
              <UIcon
                :name="line.startsWith('exclude') ? 'i-lucide-minus' : 'i-lucide-plus'"
                class="size-3 shrink-0"
                :class="line.startsWith('exclude') ? 'text-error' : 'text-success'"
              />
              <span class="min-w-0 break-words">{{ line }}</span>
            </li>
          </ul>

          <!-- What the calendar is doing, rather than what it says. -->
          <div class="mt-5 flex items-baseline justify-between gap-2">
            <p class="cq-label">
              Jobs gated by this calendar
            </p>
            <span
              v-if="held.length"
              class="cq-num text-xs text-warning"
            >{{ held.length }} held now</span>
          </div>

          <AppEmpty
            v-if="governed.length === 0"
            size="tight"
            icon="i-lucide-unlink"
            title="Nothing uses it"
            description="No schedule names this calendar, so it gates nothing. A schedule binds to it by name, in its calendar field."
          />
          <ul
            v-else
            class="flex flex-col"
          >
            <li
              v-for="row in governed"
              :key="row.trigger.trigger_id"
              class="flex h-9 items-center gap-3 text-sm"
            >
              <RouterLink
                :to="`/jobs/${encodeURIComponent(row.trigger.job_key)}`"
                class="min-w-0 flex-1 truncate font-mono text-primary hover:underline"
              >
                {{ row.trigger.job_key }}
              </RouterLink>
              <UIcon
                v-if="row.state?.suppressed_by?.includes(calendar.name)"
                name="i-lucide-lock"
                class="size-3.5 shrink-0 text-warning"
                :title="`Held right now: ${row.state.suppressed_by}`"
              />
              <span
                class="cq-num text-xs text-muted"
                :title="formatAbsolute(row.state?.next_fire_at)"
              >{{ formatRelative(row.state?.next_fire_at) }}</span>
            </li>
          </ul>
          <p class="mt-2 text-xs text-muted">
            Next fire times come from the server and are computed
            <em>through</em> this gate — which is what makes them an answer to
            "does this calendar do what I meant" rather than a restatement of
            the rules.
          </p>
        </div>
      </template>
    </div>

    <CalendarForm
      v-if="calendar"
      v-model:open="editing"
      :calendar="calendar"
    />

    <UModal
      v-model:open="confirmingDelete"
      title="Delete this calendar?"
      :description="
        governed.length
          ? `${governed.length} schedule${governed.length === 1 ? '' : 's'} still name it. They will fail to load on the next config reload.`
          : 'Nothing references it. Existing runs are unaffected.'
      "
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
            :loading="removeCalendar.isPending.value"
            @click="doDelete"
          >
            Delete
          </UButton>
        </div>
      </template>
    </UModal>
  </aside>
</template>
