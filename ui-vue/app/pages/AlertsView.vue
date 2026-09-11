<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useAlertDeliveries, useAlertsConfig } from '~/api/queries'
import type { AlertChannelConfig, AlertRuleConfig, AlertRuleOverride } from '~/api/types'
import { formatRelative } from '~/lib/format'

/**
 * Alerting: the rules, the channels they deliver through, and what went out.
 *
 * `docs/ui-screen-inventory.md` left one question open — whether the delivery
 * history stays with the rules or becomes a filterable list beside Runs. It is
 * answered here, and the answer is neither of the two options as posed.
 *
 * The audit's complaint about the React page was that it *mixed* two things:
 * configuration and a log, flat, on one page. The fix for mixing is not
 * separation into different screens — it is structure. Splitting them would
 * put a navigation step between a question and its answer, and the questions
 * here run straight across the boundary: you snooze a rule *because* of what
 * the log shows, and you read the log to find out which rule paged you.
 *
 * So one screen, three views, each a clean list of one kind of thing, all
 * three addressable:
 *
 *   /alerts             the rules, with their overrides
 *   /alerts/channels    where deliveries go, and which rules use each
 *   /alerts/deliveries  what actually went out, filterable
 *
 * Deliveries stay out of `/executions` for the same reason dead letters stayed
 * out: a job run and an alert delivery are different objects that happen to
 * share a shape. Folding them together would make "200 runs" mean two things.
 */
const route = useRoute()
const router = useRouter()

const { data: config, isPending, isError, error, refetch } = useAlertsConfig()

const view = computed<'rules' | 'channels' | 'deliveries'>(() => {
  if (route.path.startsWith('/alerts/channels')) return 'channels'
  if (route.path.startsWith('/alerts/deliveries')) return 'deliveries'
  return 'rules'
})

const selectedRule = computed(() => (route.params.ruleName as string | undefined) ?? undefined)

const rules = computed<AlertRuleConfig[]>(() => config.value?.rules ?? [])
const channels = computed<AlertChannelConfig[]>(() =>
  Object.values(config.value?.channels ?? {}).sort((a, b) => a.name.localeCompare(b.name)),
)

/** Active overrides by rule. An expired one is inert and treated as absent. */
const overrides = computed(() => {
  const now = Date.now()
  const map = new Map<string, AlertRuleOverride>()
  for (const override of config.value?.overrides ?? []) {
    if (override.expires_at && Date.parse(override.expires_at) < now) continue
    map.set(override.rule_name, override)
  }
  return map
})

/**
 * Deliveries for the counts on the rules list. Unfiltered and capped — the
 * filtered view does its own fetch with the URL's filters.
 */
const { data: recentDeliveries } = useAlertDeliveries(() => ({ limit: 200 }))

const deliveryCounts = computed(() => {
  const counts = new Map<string, { total: number; failed: number }>()
  for (const delivery of recentDeliveries.value ?? []) {
    const entry = counts.get(delivery.rule_name) ?? { total: 0, failed: 0 }
    entry.total += 1
    if (delivery.state === 'failed') entry.failed += 1
    counts.set(delivery.rule_name, entry)
  }
  return counts
})

/** Which rules name each channel — the basis for spotting an unused one. */
const channelUsers = computed(() => {
  const map = new Map<string, string[]>()
  for (const rule of rules.value) {
    for (const name of rule.channels) {
      const list = map.get(name)
      if (list) list.push(rule.name)
      else map.set(name, [rule.name])
    }
  }
  return map
})

/**
 * Channels a rule names that do not exist.
 *
 * The compiler keeps an unresolved reference verbatim and warns at fire time,
 * which means a rule can look configured and deliver nowhere. That failure is
 * silent everywhere else in the product, so it is marked here.
 */
function missingChannels(rule: AlertRuleConfig): string[] {
  const known = config.value?.channels ?? {}
  return rule.channels.filter((name) => !(name in known))
}

function describeChannel(channel: AlertChannelConfig): string {
  switch (channel.kind.type) {
    case 'shell':
      return channel.kind.command
    case 'webhook':
      return `${channel.kind.url} · ${channel.kind.timeout_secs}s`
    default:
      return channel.kind.reason
  }
}

/** Why a rule is not currently doing what the Croniqfile says. */
function overrideSummary(override: AlertRuleOverride): string {
  if (override.snooze_until) return `snoozed until ${formatRelative(override.snooze_until)}`
  if (override.enabled === false) return 'disabled'
  if (override.throttle_secs != null) return `throttled to ${override.throttle_secs}s`
  return 'overridden'
}

const TRIGGER_LABELS: Record<string, string> = {
  job_failed: 'a run fails',
  job_sla_missed: 'a run misses its SLA',
  job_missed_fire: 'a fire is missed',
}

function switchTo(target: 'rules' | 'channels' | 'deliveries') {
  void router.push(target === 'rules' ? '/alerts' : `/alerts/${target}`)
}

const VIEWS = [
  { label: 'Rules', value: 'rules' as const },
  { label: 'Channels', value: 'channels' as const },
  { label: 'Deliveries', value: 'deliveries' as const },
]

function openRule(rule: AlertRuleConfig) {
  void router.push(`/alerts/rules/${encodeURIComponent(rule.name)}`)
}

const selected = computed(() => rules.value.find((rule) => rule.name === selectedRule.value) ?? null)

/** Rules that are not currently in the state the Croniqfile declares. */
const overriddenCount = computed(() => overrides.value.size)
</script>

<template>
  <div class="flex h-full min-h-0 flex-col gap-4">
    <div class="flex flex-wrap items-center gap-3">
      <UTabs
        :model-value="view"
        :items="VIEWS"
        :content="false"
        variant="link"
        size="sm"
        @update:model-value="(value: string | number) => switchTo(value as 'rules' | 'channels' | 'deliveries')"
      />

      <div class="ml-auto flex items-center gap-3">
        <!-- A rule that is snoozed or disabled is the difference between
             "nothing is wrong" and "nothing is being reported". -->
        <RouterLink
          v-if="overriddenCount"
          to="/alerts"
          class="cq-num flex items-center gap-1.5 text-sm text-warning"
        >
          <UIcon
            name="i-lucide-bell-off"
            class="size-4"
          />
          {{ overriddenCount }} overridden
        </RouterLink>
        <span
          v-if="view === 'rules'"
          class="cq-num text-sm text-muted"
        >{{ rules.length }} rule{{ rules.length === 1 ? '' : 's' }}</span>
        <span
          v-else-if="view === 'channels'"
          class="cq-num text-sm text-muted"
        >{{ channels.length }} channel{{ channels.length === 1 ? '' : 's' }}</span>
      </div>
    </div>

    <AppLoading
      v-if="isPending"
      label="Loading the alert configuration"
    />
    <AppError
      v-else-if="isError"
      :error="error"
      :on-retry="() => refetch()"
    />

    <!-- Deliveries own their filters and their own fetch. -->
    <AlertDeliveries
      v-else-if="view === 'deliveries'"
      class="min-h-0 flex-1"
    />

    <div
      v-else-if="view === 'channels'"
      class="min-h-0 flex-1 overflow-auto rounded-lg border border-default"
    >
      <AppEmpty
        v-if="channels.length === 0"
        icon="i-lucide-radio"
        title="No channels"
        description="Alerting is configured in the Croniqfile, in an alerts { } block. Without a channel, a rule has nowhere to deliver."
      />
      <table
        v-else
        class="w-full border-collapse"
      >
        <thead class="sticky top-0 z-10 bg-default">
          <tr class="border-b border-default">
            <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
              Channel
            </th>
            <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
              Kind
            </th>
            <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
              Target
            </th>
            <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
              Used by
            </th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="channel in channels"
            :key="channel.name"
            class="cq-row border-b border-default/60"
          >
            <td class="px-[var(--cq-cell-x)] font-mono">
              {{ channel.name }}
            </td>
            <td class="px-[var(--cq-cell-x)]">
              <!-- `unknown` is a real compiled state: the Croniqfile named a
                   kind this server does not implement. The evaluator logs and
                   skips it, so the channel is configured and inert. -->
              <UBadge
                :color="channel.kind.type === 'unknown' ? 'warning' : 'neutral'"
                variant="subtle"
                size="sm"
              >
                {{ channel.kind.type }}
              </UBadge>
            </td>
            <td class="max-w-[24rem] truncate px-[var(--cq-cell-x)] font-mono text-muted">
              {{ describeChannel(channel) }}
            </td>
            <td class="px-[var(--cq-cell-x)] text-muted">
              <span v-if="channelUsers.get(channel.name)?.length">
                {{ channelUsers.get(channel.name)!.join(', ') }}
              </span>
              <span
                v-else
                title="No rule delivers through this channel"
              >unused</span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <div
      v-else
      class="flex min-h-0 flex-1 gap-4"
    >
      <div class="min-w-0 flex-1 overflow-auto rounded-lg border border-default">
        <AppEmpty
          v-if="rules.length === 0"
          icon="i-lucide-bell-off"
          title="No alert rules"
          description="Alerting is configured in the Croniqfile, in an alerts { } block — a channel to deliver through and a rule saying what to report. Nothing here is editable through the API; overrides are."
        />

        <table
          v-else
          class="w-full border-collapse"
        >
          <thead class="sticky top-0 z-10 bg-default">
            <tr class="border-b border-default">
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
                Rule
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
                Fires when
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
                Jobs
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
                Channels
              </th>
              <th
                v-if="!selectedRule"
                class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-right"
              >
                Sent
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
                State
              </th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="rule in rules"
              :key="rule.name"
              :class="[
                'cq-row cursor-pointer border-b border-default/60 transition-colors hover:bg-elevated',
                rule.name === selectedRule && 'bg-elevated',
              ]"
              @click="openRule(rule)"
            >
              <td class="max-w-[14rem] truncate px-[var(--cq-cell-x)] font-mono text-primary">
                {{ rule.name }}
              </td>
              <td class="px-[var(--cq-cell-x)] text-muted">
                {{ TRIGGER_LABELS[rule.trigger] ?? rule.trigger }}
              </td>
              <td class="max-w-[12rem] truncate px-[var(--cq-cell-x)] font-mono text-muted">
                {{ rule.job_key_glob }}
              </td>
              <td class="max-w-[14rem] truncate px-[var(--cq-cell-x)] font-mono text-muted">
                {{ rule.channels.join(' ') || '—' }}
                <UIcon
                  v-if="missingChannels(rule).length"
                  name="i-lucide-triangle-alert"
                  class="size-3.5 text-error"
                  :title="`No such channel: ${missingChannels(rule).join(', ')} — this rule delivers nowhere`"
                />
              </td>
              <td
                v-if="!selectedRule"
                class="cq-num px-[var(--cq-cell-x)] text-right text-muted"
              >
                {{ deliveryCounts.get(rule.name)?.total ?? 0 }}
                <span
                  v-if="deliveryCounts.get(rule.name)?.failed"
                  class="text-error"
                >· {{ deliveryCounts.get(rule.name)!.failed }} failed</span>
              </td>
              <td class="px-[var(--cq-cell-x)]">
                <UBadge
                  v-if="overrides.get(rule.name)"
                  color="warning"
                  variant="subtle"
                  size="sm"
                  :title="overrides.get(rule.name)!.note"
                >
                  {{ overrideSummary(overrides.get(rule.name)!) }}
                </UBadge>
                <span
                  v-else
                  class="text-sm text-muted"
                >active</span>
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <AlertRuleDetail
        v-if="selectedRule"
        :rule="selected"
        :rule-name="selectedRule"
        :override="overrides.get(selectedRule) ?? null"
        :missing-channels="selected ? missingChannels(selected) : []"
        class="w-[30rem] shrink-0"
        @close="router.push('/alerts')"
      />
    </div>
  </div>
</template>
