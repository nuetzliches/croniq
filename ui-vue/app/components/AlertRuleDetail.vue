<script setup lang="ts">
import { computed, ref } from 'vue'
import { ApiError } from '~/api/client'
import {
  useAlertDeliveries,
  useClearOverride,
  useDisableRule,
  useSnoozeRule,
  useThrottleRule,
} from '~/api/queries'
import type { AlertRuleConfig, AlertRuleOverride } from '~/api/types'
import { formatAbsolute, formatRelative } from '~/lib/format'

/**
 * One alert rule: what the Croniqfile says, what an operator has done to it,
 * and what it has actually sent.
 *
 * The three are together because they only mean anything together. A rule's
 * configuration says what *should* happen; the override says why it currently
 * doesn't; the deliveries say what did. Reading any one of them alone is how
 * you end up snoozing a rule that was already disabled.
 */
const props = defineProps<{
  rule: AlertRuleConfig | null
  ruleName: string
  override: AlertRuleOverride | null
  missingChannels: string[]
}>()
const emit = defineEmits<{ close: [] }>()

const { data: deliveries } = useAlertDeliveries(() => ({ rule_name: props.ruleName, limit: 20 }))

const snooze = useSnoozeRule()
const disable = useDisableRule()
const throttle = useThrottleRule()
const clear = useClearOverride()

const actionError = ref<string | null>(null)
/** Which override form is open; only one intent applies at a time. */
const composing = ref<'snooze' | 'disable' | 'throttle' | null>(null)
const note = ref('')
const duration = ref('1h')

const pending = computed(
  () =>
    snooze.isPending.value ||
    disable.isPending.value ||
    throttle.isPending.value ||
    clear.isPending.value,
)

async function run(fn: () => Promise<unknown>) {
  actionError.value = null
  try {
    await fn()
    composing.value = null
    note.value = ''
  } catch (caught) {
    const body = caught instanceof ApiError ? (caught.body as { message?: string }) : undefined
    actionError.value = body?.message ?? (caught as Error).message ?? 'The server refused that.'
  }
}

/**
 * Durations are entered as `30m` / `2h` / `1d` and sent as an instant.
 *
 * The snooze endpoint takes a deadline, not a length, because the deadline is
 * what it stores and auto-clears on. Converting here rather than asking for a
 * timestamp is the difference between "quiet for an hour" and doing date
 * arithmetic in your head at 3am.
 */
function deadlineFrom(input: string): string | null {
  const match = /^(\d+)\s*(m|h|d)$/i.exec(input.trim())
  if (!match) return null
  const value = Number.parseInt(match[1]!, 10)
  const ms = { m: 60_000, h: 3_600_000, d: 86_400_000 }[match[2]!.toLowerCase() as 'm' | 'h' | 'd']
  return new Date(Date.now() + value * ms).toISOString()
}

function submit() {
  const trimmedNote = note.value.trim()
  if (!trimmedNote) {
    actionError.value = 'A note is required — the next person needs to know why it is quiet.'
    return
  }

  if (composing.value === 'snooze') {
    const until = deadlineFrom(duration.value)
    if (!until) {
      actionError.value = 'Use a duration like 30m, 2h or 1d.'
      return
    }
    void run(() => snooze.mutateAsync({ name: props.ruleName, until, note: trimmedNote }))
    return
  }

  if (composing.value === 'disable') {
    // Open-ended when no duration is given: disabling indefinitely is a real
    // intent, and forcing a deadline would turn it into a long snooze.
    const expires = duration.value.trim() ? deadlineFrom(duration.value) : null
    if (duration.value.trim() && !expires) {
      actionError.value = 'Use a duration like 30m, 2h or 1d — or leave it empty for open-ended.'
      return
    }
    void run(() =>
      disable.mutateAsync({ name: props.ruleName, note: trimmedNote, expires_at: expires }),
    )
    return
  }

  if (composing.value === 'throttle') {
    if (!/^\d+\s*(s|m|h)$/i.test(duration.value.trim())) {
      actionError.value = 'Use a throttle window like 30s, 15m or 2h.'
      return
    }
    void run(() =>
      throttle.mutateAsync({
        name: props.ruleName,
        throttle: duration.value.trim(),
        note: trimmedNote,
        expires_at: null,
      }),
    )
  }
}

function startComposing(intent: 'snooze' | 'disable' | 'throttle') {
  composing.value = intent
  actionError.value = null
  duration.value = intent === 'throttle' ? '30m' : '1h'
}

const facts = computed(() => {
  const rule = props.rule
  if (!rule) return []
  return [
    { label: 'Jobs', value: rule.job_key_glob, mono: true },
    { label: 'Channels', value: rule.channels.join(' ') || '—', mono: true },
    { label: 'Min attempts', value: String(rule.min_attempts) },
    ...(rule.dead_letter_only ? [{ label: 'Scope', value: 'dead-lettered runs only' }] : []),
    ...(rule.throttle ? [{ label: 'Throttle', value: rule.throttle, mono: true }] : []),
    ...(rule.expected_within
      ? [{ label: 'Expected within', value: rule.expected_within, mono: true }]
      : []),
  ]
})

const recent = computed(() => deliveries.value ?? [])
</script>

<template>
  <aside
    class="flex min-h-0 flex-col overflow-hidden rounded-lg border border-default"
    aria-label="Alert rule detail"
  >
    <header class="flex shrink-0 items-center gap-2 border-b border-default px-4 py-3">
      <span class="min-w-0 flex-1 truncate font-mono text-sm text-primary">{{ ruleName }}</span>
      <UButton
        icon="i-lucide-x"
        color="neutral"
        variant="ghost"
        size="xs"
        aria-label="Close alert rule detail"
        @click="emit('close')"
      />
    </header>

    <div class="min-h-0 flex-1 overflow-y-auto">
      <AppEmpty
        v-if="!rule"
        size="tight"
        icon="i-lucide-search-x"
        title="No such rule"
        :description="`Nothing in the alerts block is named ${ruleName}. It may have been removed from the Croniqfile — alerts are read at boot, so that needs a restart to take effect.`"
      />

      <template v-else>
        <div class="flex flex-wrap items-center gap-1.5 border-b border-default px-4 py-3">
          <UButton
            icon="i-lucide-bell-off"
            color="neutral"
            variant="subtle"
            size="xs"
            @click="startComposing('snooze')"
          >
            Snooze
          </UButton>
          <UButton
            icon="i-lucide-gauge"
            color="neutral"
            variant="subtle"
            size="xs"
            @click="startComposing('throttle')"
          >
            Throttle
          </UButton>
          <UButton
            icon="i-lucide-ban"
            color="neutral"
            variant="subtle"
            size="xs"
            @click="startComposing('disable')"
          >
            Disable
          </UButton>
          <UButton
            v-if="override"
            icon="i-lucide-undo-2"
            color="neutral"
            variant="ghost"
            size="xs"
            class="ml-auto"
            :loading="clear.isPending.value"
            title="Return the rule to what the Croniqfile says"
            @click="run(() => clear.mutateAsync(ruleName))"
          >
            Clear override
          </UButton>
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

          <!-- The override form. One intent at a time, because the server
               replaces the override wholesale rather than merging. -->
          <div
            v-if="composing"
            class="mb-4 flex flex-col gap-3 rounded-lg border border-default p-3"
          >
            <p class="cq-label">
              {{
                composing === 'snooze'
                  ? 'Snooze this rule'
                  : composing === 'throttle'
                    ? 'Replace the throttle window'
                    : 'Disable this rule'
              }}
            </p>
            <UFormField
              :label="composing === 'throttle' ? 'Window' : 'For'"
              :description="
                composing === 'throttle'
                  ? 'At most one delivery per window, replacing what the Croniqfile says.'
                  : composing === 'disable'
                    ? 'Leave empty to disable open-ended.'
                    : 'How long it stays quiet. It clears itself afterwards.'
              "
            >
              <UInput
                v-model="duration"
                class="w-32 font-mono"
                :placeholder="composing === 'throttle' ? '30m' : '1h'"
                aria-label="Duration"
              />
            </UFormField>
            <UFormField
              label="Note"
              description="Required. Whoever finds this rule quiet next needs to know why."
              required
            >
              <UInput
                v-model="note"
                class="w-full"
                placeholder="Known flapping while the upstream API is down — ticket OPS-412"
                aria-label="Reason for the override"
              />
            </UFormField>
            <div class="flex justify-end gap-2">
              <UButton
                color="neutral"
                variant="ghost"
                size="xs"
                @click="composing = null"
              >
                Cancel
              </UButton>
              <UButton
                size="xs"
                :loading="pending"
                @click="submit"
              >
                Apply
              </UButton>
            </div>
          </div>

          <!-- A rule that is not doing what the file says is the single most
               important fact about it, so it sits above the configuration. -->
          <UAlert
            v-if="override"
            class="mb-4"
            color="warning"
            variant="subtle"
            icon="i-lucide-bell-off"
            :title="
              override.snooze_until
                ? `Snoozed until ${formatAbsolute(override.snooze_until)}`
                : override.enabled === false
                  ? 'Disabled'
                  : `Throttled to ${override.throttle_secs}s`
            "
            :description="`${override.note} — set ${formatRelative(override.set_at)}${
              override.expires_at ? `, clears ${formatRelative(override.expires_at)}` : ''
            }`"
          />

          <UAlert
            v-if="missingChannels.length"
            class="mb-4"
            color="error"
            variant="subtle"
            icon="i-lucide-triangle-alert"
            title="This rule delivers nowhere"
            :description="`It names ${missingChannels.join(', ')}, and no such channel is configured. The rule evaluates, finds nothing to deliver through, and logs at fire time — so it looks configured and is silent.`"
          />

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

          <div class="mt-5 flex items-baseline justify-between gap-2">
            <p class="cq-label">
              Recent deliveries
            </p>
            <RouterLink
              :to="`/alerts/deliveries?rule=${encodeURIComponent(ruleName)}`"
              class="text-xs text-primary hover:underline"
            >
              View all
            </RouterLink>
          </div>

          <AppEmpty
            v-if="recent.length === 0"
            size="tight"
            icon="i-lucide-bell"
            title="Never fired"
            description="Either nothing matched it, or it is newer than the log."
          />
          <ul
            v-else
            class="flex flex-col"
          >
            <li
              v-for="delivery in recent"
              :key="delivery.delivery_id"
              class="flex h-8 items-center gap-2 text-sm"
            >
              <span
                class="size-1.5 shrink-0 rounded-full"
                :class="
                  delivery.state === 'delivered'
                    ? 'bg-success'
                    : delivery.state === 'failed'
                      ? 'bg-error'
                      : 'bg-dimmed'
                "
                :title="delivery.state"
              />
              <RouterLink
                :to="`/jobs/${encodeURIComponent(delivery.job_key)}`"
                class="min-w-0 flex-1 truncate font-mono text-primary hover:underline"
              >
                {{ delivery.job_key }}
              </RouterLink>
              <span
                class="cq-num text-xs text-muted"
                :title="formatAbsolute(delivery.fired_at)"
              >{{ formatRelative(delivery.fired_at) }}</span>
            </li>
          </ul>
        </div>
      </template>
    </div>
  </aside>
</template>
