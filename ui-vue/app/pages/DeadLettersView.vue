<script setup lang="ts">
import { computed, ref } from 'vue'
import { ApiError } from '~/api/client'
import { useDeadLetters, useDeleteDeadLetter, useReplayDeadLetter } from '~/api/queries'
import type { DeadLetter } from '~/api/types'
import { formatAbsolute, formatRelative, shortId } from '~/lib/format'

/**
 * Dead letters — the work queue.
 *
 * It kept its own screen rather than becoming a state filter on Runs
 * (docs/ui-screen-inventory.md) because it is the only surface in the product
 * that is a *to-do list*: every row is waiting for a person to decide replay
 * or discard. Folding it into a browsing list would mean setting a filter
 * before you could see that anything was waiting.
 */
const { data, isPending, isError, error, refetch } = useDeadLetters()
const replay = useReplayDeadLetter()
const remove = useDeleteDeadLetter()

const rows = computed<DeadLetter[]>(() => data.value ?? [])
const selectedId = ref<string | null>(null)
const selected = computed(() => rows.value.find((row) => row.id === selectedId.value) ?? null)

/** What the last replay attempt said, when it said no. */
const replayError = ref<string | null>(null)

/**
 * Replay can be refused, and the refusal is a decision rather than a fault:
 * the server rejects one whose logical fire time is older than the job's
 * `dead_letter_replay_max_age`. Reporting the server's own words is the
 * difference between "replay failed" and knowing why.
 */
async function doReplay(id: string) {
  replayError.value = null
  try {
    await replay.mutateAsync(id)
    if (selectedId.value === id) selectedId.value = null
  } catch (caught) {
    const body = caught instanceof ApiError ? (caught.body as { message?: string }) : undefined
    replayError.value = body?.message ?? (caught as Error).message ?? 'Replay was refused.'
  }
}

async function doDelete(id: string) {
  await remove.mutateAsync(id)
  if (selectedId.value === id) selectedId.value = null
}

const expiring = (row: DeadLetter) => Boolean(row.expires_at)
</script>

<template>
  <div class="flex h-full min-h-0 flex-col gap-4">
    <div class="flex items-center gap-3">
      <p class="text-sm text-muted">
        Runs that exhausted their retries. Each one is waiting for a decision.
      </p>
      <span class="cq-num ml-auto text-sm text-muted">{{ rows.length }} pending</span>
    </div>

    <UAlert
      v-if="replayError"
      color="warning"
      variant="subtle"
      icon="i-lucide-shield-alert"
      title="Replay refused"
      :description="replayError"
      role="alert"
      close
      @update:open="replayError = null"
    />

    <div class="flex min-h-0 flex-1 gap-4">
      <div class="min-w-0 flex-1 overflow-auto rounded-lg border border-default">
        <AppLoading
          v-if="isPending"
          label="Loading dead letters"
        />
        <AppError
          v-else-if="isError"
          :error="error"
          :on-retry="() => refetch()"
        />
        <AppEmpty
          v-else-if="rows.length === 0"
          icon="i-lucide-check"
          title="Nothing waiting"
          description="No run has exhausted its retries. Dead letters appear here when one does, and stay until you replay or discard them."
        />

        <table
          v-else
          class="w-full border-collapse"
        >
          <thead class="sticky top-0 z-10 bg-default">
            <tr class="border-b border-default">
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
                Job
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-left">
                Reason
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-right">
                Attempt
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-right">
                Died
              </th>
              <th class="cq-label px-[var(--cq-cell-x)] py-[var(--cq-cell-y)] text-right">
                Expires
              </th>
              <th class="w-24" />
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="row in rows"
              :key="row.id"
              :class="[
                'cq-row',
                'cursor-pointer border-b border-default/60 transition-colors hover:bg-elevated',
                row.id === selectedId && 'bg-elevated',
              ]"
              @click="selectedId = row.id"
            >
              <td class="max-w-[16rem] truncate px-[var(--cq-cell-x)] font-mono text-primary">
                {{ row.job_key }}
              </td>
              <td class="max-w-[20rem] truncate px-[var(--cq-cell-x)] text-muted">
                {{ row.dead_reason }}
              </td>
              <td class="cq-num px-[var(--cq-cell-x)] text-right">
                {{ row.attempt }}
              </td>
              <td
                class="cq-num px-[var(--cq-cell-x)] text-right text-muted"
                :title="formatAbsolute(row.created_at)"
              >
                {{ formatRelative(row.created_at) }}
              </td>
              <td
                class="cq-num px-[var(--cq-cell-x)] text-right text-muted"
                :title="formatAbsolute(row.expires_at)"
              >
                {{ expiring(row) ? formatRelative(row.expires_at) : 'never' }}
              </td>
              <td class="px-[var(--cq-cell-x)] text-right">
                <div class="flex items-center justify-end gap-1">
                  <UButton
                    icon="i-lucide-rotate-cw"
                    color="neutral"
                    variant="ghost"
                    size="xs"
                    :aria-label="`Replay ${row.job_key}`"
                    title="Replay"
                    :loading="replay.isPending.value"
                    @click.stop="doReplay(row.id)"
                  />
                  <UButton
                    icon="i-lucide-trash-2"
                    color="error"
                    variant="ghost"
                    size="xs"
                    :aria-label="`Discard the dead letter for ${row.job_key}`"
                    title="Discard"
                    :loading="remove.isPending.value"
                    @click.stop="doDelete(row.id)"
                  />
                </div>
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <aside
        v-if="selected"
        class="flex w-[26rem] shrink-0 flex-col overflow-hidden rounded-lg border border-default"
        aria-label="Dead letter detail"
      >
        <header class="flex shrink-0 items-center gap-2 border-b border-default px-4 py-3">
          <span class="min-w-0 flex-1 truncate font-mono text-sm text-primary">{{
            selected.job_key
          }}</span>
          <UButton
            icon="i-lucide-x"
            color="neutral"
            variant="ghost"
            size="xs"
            aria-label="Close dead letter detail"
            @click="selectedId = null"
          />
        </header>

        <div class="min-h-0 flex-1 overflow-y-auto p-4">
          <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
            <dt class="text-muted">
              Run
            </dt>
            <dd
              class="truncate text-right font-mono"
              :title="selected.execution_id"
            >
              {{ shortId(selected.execution_id) }}
            </dd>
            <dt class="text-muted">
              Reason
            </dt>
            <dd class="text-right">
              {{ selected.dead_reason }}
            </dd>
            <dt class="text-muted">
              Attempt
            </dt>
            <dd class="text-right">
              {{ selected.attempt }}
            </dd>
            <dt class="text-muted">
              Fired
            </dt>
            <dd class="text-right">
              {{ formatAbsolute(selected.fire_at) }}
            </dd>
            <!-- The logical trigger instant, held across retries and replay.
                 It is what the stale-replay guard measures against, so it
                 belongs beside the replay button. -->
            <dt class="text-muted">
              Scheduled for
            </dt>
            <dd class="text-right">
              {{ formatAbsolute(selected.scheduled_for) }}
            </dd>
            <dt class="text-muted">
              Expires
            </dt>
            <dd class="text-right">
              {{ expiring(selected) ? formatAbsolute(selected.expires_at) : 'never' }}
            </dd>
          </dl>

          <div class="mt-4">
            <p class="cq-label mb-1.5 text-error">
              Error
            </p>
            <pre class="overflow-x-auto rounded-md border border-default bg-elevated p-3 font-mono text-xs whitespace-pre-wrap">{{ selected.error }}</pre>
          </div>

          <div
            v-if="Object.keys(selected.metadata ?? {}).length"
            class="mt-4"
          >
            <p class="cq-label mb-1.5">
              Metadata
            </p>
            <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 font-mono text-xs">
              <template
                v-for="(value, key) in selected.metadata"
                :key="key"
              >
                <dt class="text-muted">
                  {{ key }}
                </dt>
                <dd class="truncate text-right">
                  {{ value }}
                </dd>
              </template>
            </dl>
          </div>

          <div class="mt-4 flex gap-2">
            <UButton
              icon="i-lucide-rotate-cw"
              :loading="replay.isPending.value"
              @click="doReplay(selected.id)"
            >
              Replay
            </UButton>
            <UButton
              color="error"
              variant="subtle"
              icon="i-lucide-trash-2"
              :loading="remove.isPending.value"
              @click="doDelete(selected.id)"
            >
              Discard
            </UButton>
          </div>
        </div>
      </aside>
    </div>
  </div>
</template>
