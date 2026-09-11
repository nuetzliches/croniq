<script setup lang="ts">
import { computed, ref } from 'vue'
import { useDeleteRunner } from '~/api/queries'
import { useRunnersStream } from '~/composables/useRunnersStream'
import { formatAbsolute, formatRelative } from '~/lib/format'

/**
 * The runner fleet.
 *
 * The audit's complaint about its predecessor: master/detail applied
 * uniformly, so with a single runner ~85% of the viewport was an empty pane
 * saying "Select a runner". Everything a runner detail showed is either a
 * field that fits in its row or a list of runs — and runs now live in one
 * place, so the row links there instead of re-rendering them here
 * (docs/ui-screen-inventory.md).
 */
const { runners, connected, received } = useRunnersStream()
const removeRunner = useDeleteRunner()

const tagFilter = ref('')

/** Every distinct tag, with how many runners carry it. */
const tags = computed(() => {
  const counts = new Map<string, number>()
  for (const runner of runners.value) {
    for (const tag of runner.tags) counts.set(tag, (counts.get(tag) ?? 0) + 1)
  }
  return [...counts.entries()].sort(([a], [b]) => a.localeCompare(b))
})

const rows = computed(() =>
  tagFilter.value
    ? runners.value.filter((runner) => runner.tags.includes(tagFilter.value))
    : runners.value,
)

const online = computed(() => runners.value.filter((r) => r.status === 'online').length)

/**
 * Removing a runner is not a graceful drain, and the wording says so: the
 * server keeps its in-flight executions claimed until the lease expires and
 * they time out.
 */
async function remove(runnerId: string) {
  await removeRunner.mutateAsync(runnerId)
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col gap-4">
    <div class="flex flex-wrap items-center gap-2">
      <UButton
        :variant="tagFilter ? 'ghost' : 'subtle'"
        color="neutral"
        size="sm"
        @click="tagFilter = ''"
      >
        All {{ runners.length }}
      </UButton>
      <UButton
        v-for="[tag, count] in tags"
        :key="tag"
        :variant="tagFilter === tag ? 'subtle' : 'ghost'"
        color="neutral"
        size="sm"
        class="font-mono"
        @click="tagFilter = tagFilter === tag ? '' : tag"
      >
        {{ tag }} {{ count }}
      </UButton>

      <div class="ml-auto flex items-center gap-2">
        <!-- The stream's own state. A fleet list that has silently stopped
             updating looks exactly like a fleet that has stopped changing. -->
        <span
          class="flex items-center gap-1.5 text-xs"
          :class="connected ? 'text-success' : 'text-muted'"
          role="status"
          aria-live="polite"
        >
          <UIcon
            :name="connected ? 'i-lucide-wifi' : 'i-lucide-wifi-off'"
            class="size-3.5"
          />
          {{ connected ? 'live' : 'reconnecting' }}
        </span>
        <span class="cq-num text-sm text-muted">{{ online }} online</span>
      </div>
    </div>

    <div class="min-h-0 flex-1 overflow-auto rounded-lg border border-default">
      <AppLoading
        v-if="!received"
        label="Connecting to the runner stream"
      />
      <AppEmpty
        v-else-if="rows.length === 0"
        icon="i-lucide-cpu"
        :title="tagFilter ? 'No runners with that tag' : 'No runners connected'"
        :description="
          tagFilter
            ? 'No connected runner carries this tag.'
            : 'Runners register themselves on their first poll. Start one with a scoped API key and it appears here.'
        "
      >
        <template
          v-if="tagFilter"
          #action
        >
          <UButton
            variant="subtle"
            color="neutral"
            @click="tagFilter = ''"
          >
            Clear filter
          </UButton>
        </template>
      </AppEmpty>

      <table
        v-else
        class="w-full border-collapse"
      >
        <thead class="sticky top-0 z-10 bg-default">
          <tr class="border-b border-default">
            <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
              Status
            </th>
            <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
              Runner
            </th>
            <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
              Tags
            </th>
            <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
              Capabilities
            </th>
            <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-right">
              In flight
            </th>
            <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-right">
              Last poll
            </th>
            <th class="w-20" />
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="runner in rows"
            :key="runner.runner_id"
            class="cq-row border-b border-default/60"
          >
            <td class="px-[var(--cq-cell-x)]">
              <StatusPill :state="runner.status" />
            </td>
            <td class="max-w-[18rem] truncate px-[var(--cq-cell-x)] font-mono">
              {{ runner.runner_id }}
            </td>
            <td class="max-w-[14rem] truncate px-[var(--cq-cell-x)] font-mono text-muted">
              {{ runner.tags.join(' ') || '—' }}
            </td>
            <td class="max-w-[14rem] truncate px-[var(--cq-cell-x)] font-mono text-muted">
              {{ runner.capabilities.join(' ') || '—' }}
            </td>
            <td class="cq-num px-[var(--cq-cell-x)] text-right">
              <span :class="runner.inflight >= runner.max_inflight && 'text-warning'">
                {{ runner.inflight }}/{{ runner.max_inflight }}
              </span>
            </td>
            <td
              class="cq-num px-[var(--cq-cell-x)] text-right text-muted"
              :title="formatAbsolute(runner.last_poll_at)"
            >
              {{ formatRelative(runner.last_poll_at) }}
            </td>
            <td class="px-[var(--cq-cell-x)] text-right">
              <div class="flex items-center justify-end gap-1">
                <!-- Into the one run list, filtered — rather than a second
                     executions table living here. -->
                <UButton
                  :to="`/executions?runner_id=${encodeURIComponent(runner.runner_id)}`"
                  icon="i-lucide-list"
                  color="neutral"
                  variant="ghost"
                  size="xs"
                  :aria-label="`Runs on ${runner.runner_id}`"
                  title="Runs on this runner"
                />
                <UButton
                  icon="i-lucide-trash-2"
                  color="error"
                  variant="ghost"
                  size="xs"
                  :aria-label="`Remove ${runner.runner_id}`"
                  title="Remove — in-flight work stays claimed until its lease expires"
                  :loading="removeRunner.isPending.value"
                  @click="remove(runner.runner_id)"
                />
              </div>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>
