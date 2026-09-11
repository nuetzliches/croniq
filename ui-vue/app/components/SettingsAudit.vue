<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useAuditEvents, useUsers } from '~/api/queries'
import { formatAbsolute, formatRelative, shortId } from '~/lib/format'

/**
 * Who did what.
 *
 * One home, not two: the React tree rendered this both here and inside the job
 * detail, which meant two filters, two limits and two chances to disagree. The
 * job detail links here now.
 *
 * Filters are in the URL, like every other list — so "everything this person
 * did" and "every deletion" are links.
 *
 * Each row links to what it touched where that is possible. An audit log you
 * cannot follow is a log you read once and then go looking in the database.
 */
const route = useRoute()
const router = useRouter()

const filters = computed(() => ({
  actor_id: (route.query.actor as string) || '',
  target_type: (route.query.target as string) || '',
  action: (route.query.action as string) || '',
}))

const { data, isPending, isError, error, refetch } = useAuditEvents(() => ({
  actor_id: filters.value.actor_id || undefined,
  target_type: filters.value.target_type || undefined,
  action: filters.value.action || undefined,
}))

const rows = computed(() => data.value ?? [])

/**
 * Actor ids are UUIDs, and a column of UUIDs answers "who did what" with
 * "someone did what". The users list is already loaded on this screen, so the
 * id resolves to a name — falling back to a short id for an actor who is no
 * longer a user, which is precisely the case where the log matters most.
 */
const { data: users } = useUsers()
const namesById = computed(
  () => new Map((users.value ?? []).map((user) => [user.user_id, user.username])),
)
function actorName(actorId: string): string {
  return namesById.value.get(actorId) ?? shortId(actorId)
}

const hasFilters = computed(() =>
  Boolean(filters.value.actor_id || filters.value.target_type || filters.value.action),
)

function setFilter(key: 'actor' | 'target' | 'action', value: string) {
  const query = { ...route.query }
  if (value) query[key] = value
  else delete query[key]
  void router.replace({ path: '/settings/audit', query })
}

/** The target types the server actually emits, for the filter menu. */
const targetTypes = computed(() =>
  [...new Set(rows.value.map((event) => event.target_type))].filter(Boolean).sort(),
)

/**
 * Where a row points.
 *
 * Only the target types that have a screen; anything else stays plain text
 * rather than becoming a link that 404s.
 */
function targetLink(targetType: string, targetId: string | null): string | null {
  if (!targetId) return null
  switch (targetType) {
    case 'job':
      return `/jobs/${encodeURIComponent(targetId)}`
    case 'calendar':
      return `/calendars/${encodeURIComponent(targetId)}`
    case 'execution':
      return `/executions/${encodeURIComponent(targetId)}`
    case 'alert_rule':
      return `/alerts/rules/${encodeURIComponent(targetId)}`
    default:
      return null
  }
}

/**
 * `diff_json` is a JSON string when present. It is shown as a title rather
 * than a column: it is long, it is occasionally the only thing that matters,
 * and a column of truncated JSON is neither readable nor scannable.
 */
function diffTitle(diff: string | null): string | undefined {
  if (!diff) return undefined
  try {
    return JSON.stringify(JSON.parse(diff), null, 2)
  } catch {
    return diff
  }
}
</script>

<template>
  <div class="flex min-h-0 flex-col gap-3">
    <div class="flex flex-wrap items-center gap-2">
      <UInput
        :model-value="filters.action"
        placeholder="Action…"
        icon="i-lucide-search"
        aria-label="Filter the audit log by action"
        class="w-48"
        @update:model-value="(value: string) => setFilter('action', value)"
      />
      <USelectMenu
        :model-value="filters.target_type || undefined"
        :items="targetTypes"
        placeholder="Any target"
        aria-label="Filter the audit log by target type"
        class="w-44"
        @update:model-value="(value: string) => setFilter('target', value ?? '')"
      />
      <UInput
        :model-value="filters.actor_id"
        placeholder="Actor…"
        icon="i-lucide-user"
        aria-label="Filter the audit log by actor"
        class="w-48"
        @update:model-value="(value: string) => setFilter('actor', value)"
      />
      <UButton
        v-if="hasFilters"
        variant="ghost"
        color="neutral"
        icon="i-lucide-x"
        size="sm"
        @click="router.replace('/settings/audit')"
      >
        Clear
      </UButton>
      <span class="cq-num ml-auto text-sm text-muted">{{ rows.length }} event{{ rows.length === 1 ? '' : 's' }}</span>
    </div>

    <div class="min-h-0 flex-1 overflow-auto rounded-lg border border-default">
      <AppLoading
        v-if="isPending"
        label="Loading the audit log"
      />
      <AppError
        v-else-if="isError"
        :error="error"
        :on-retry="() => refetch()"
      />
      <AppEmpty
        v-else-if="rows.length === 0"
        icon="i-lucide-scroll-text"
        :title="hasFilters ? 'Nothing matches' : 'Nothing recorded'"
        :description="
          hasFilters
            ? 'No audit event matches these filters.'
            : 'Every change made through the API is recorded here. An empty log means nothing has been changed yet.'
        "
      />

      <table
        v-else
        class="w-full border-collapse"
      >
        <thead class="sticky top-0 z-10 bg-default">
          <tr class="border-b border-default">
            <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
              When
            </th>
            <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
              Who
            </th>
            <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
              Did
            </th>
            <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
              To
            </th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="event in rows"
            :key="event.event_id"
            class="cq-row border-b border-default/60"
          >
            <td
              class="cq-num px-[var(--cq-cell-x)] text-muted"
              :title="formatAbsolute(event.created_at)"
            >
              {{ formatRelative(event.created_at) }}
            </td>
            <td class="max-w-[12rem] truncate px-[var(--cq-cell-x)]">
              <!-- Clicking a name filters to that person: "what did they
                   change" is the question this log exists for. -->
              <UButton
                v-if="event.actor_id"
                variant="link"
                color="neutral"
                size="xs"
                class="p-0 font-mono"
                :title="`Only events by ${event.actor_id}`"
                @click="setFilter('actor', event.actor_id!)"
              >
                {{ actorName(event.actor_id) }}
              </UButton>
              <span
                v-else
                class="text-muted"
                title="No user record — an API-key session"
              >system</span>
            </td>
            <td class="max-w-[16rem] truncate px-[var(--cq-cell-x)] font-mono">
              <span :title="diffTitle(event.diff_json)">{{ event.action }}</span>
            </td>
            <td class="max-w-[20rem] truncate px-[var(--cq-cell-x)] font-mono text-muted">
              <span class="mr-1.5">{{ event.target_type }}</span>
              <RouterLink
                v-if="targetLink(event.target_type, event.target_id)"
                :to="targetLink(event.target_type, event.target_id)!"
                class="text-primary hover:underline"
                :title="event.target_id ?? ''"
              >
                {{ event.target_id }}
              </RouterLink>
              <span
                v-else-if="event.target_id"
                :title="event.target_id"
              >{{ shortId(event.target_id) }}</span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>
