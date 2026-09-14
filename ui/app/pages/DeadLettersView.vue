<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ApiError } from '~/api/client'
import {
  useBulkDeleteDeadLetters,
  useDeadLetterCount,
  useDeadLetters,
  useDeleteDeadLetter,
  useReplayDeadLetter,
} from '~/api/queries'
import type { DeadLetter } from '~/api/types'
import { formatAbsolute, formatRelative, shortId } from '~/lib/format'
import ConfirmModal from '~/components/ConfirmModal.vue'

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

/**
 * How many there are, which is not how many are on screen.
 *
 * The list endpoint applies a page limit — 50 by default — so counting the
 * rows here said "50 pending" for a queue of any size, and the discard-all
 * dialog offered to remove "all 50" while the request cleared the lot
 * (issue #661).
 */
const total = useDeadLetterCount()

const rows = computed<DeadLetter[]>(() => data.value ?? [])
const selectedId = ref<string | null>(null)
const selected = computed(() => rows.value.find((row) => row.id === selectedId.value) ?? null)

/** What the last replay attempt said, when it said no. */
const replayError = ref<string | null>(null)
/** And what a bulk action did, which is a number worth reporting. */
const bulkNotice = ref<string | null>(null)

const bulkDelete = useBulkDeleteDeadLetters()

/**
 * Which rows are picked for a bulk action.
 *
 * A queue of things waiting for a decision needs a way to make the same
 * decision about several at once — the shipping dashboard had it and this
 * screen did not, which is the one gap that was left against the capability
 * list in `docs/ui-screen-inventory.md`.
 *
 * Held by id rather than by index, so a row arriving or leaving under the
 * selection cannot silently move it onto a different dead letter.
 */
const picked = ref(new Set<string>())

const pickedRows = computed(() => rows.value.filter((row) => picked.value.has(row.id)))
const allPicked = computed(() => rows.value.length > 0 && pickedRows.value.length === rows.value.length)

function togglePick(id: string) {
  const next = new Set(picked.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  picked.value = next
}

function toggleAll() {
  picked.value = allPicked.value ? new Set() : new Set(rows.value.map((row) => row.id))
}

// Rows that have gone — replayed by someone else, swept by retention — must
// not stay picked, or a later bulk action would name ids the server no longer
// has.
watch(rows, (next) => {
  const live = new Set(next.map((row) => row.id))
  const surviving = [...picked.value].filter((id) => live.has(id))
  if (surviving.length !== picked.value.size) picked.value = new Set(surviving)
})

const confirmingBulk = ref<'picked' | 'all' | null>(null)

async function runBulk() {
  const intent = confirmingBulk.value
  if (!intent) return
  replayError.value = null
  bulkNotice.value = null
  try {
    const result = await bulkDelete.mutateAsync(
      intent === 'picked' ? { ids: pickedRows.value.map((row) => row.id) } : { all: true },
    )
    // The count, not a bare "done": a bulk delete that matched nothing looks
    // exactly like one that worked.
    bulkNotice.value = `Discarded ${result.deleted} dead letter${result.deleted === 1 ? '' : 's'}.`
    picked.value = new Set()
    selectedId.value = null
  } catch (caught) {
    const body = caught instanceof ApiError ? (caught.body as { message?: string }) : undefined
    replayError.value = body?.message ?? (caught as Error).message ?? 'The server refused that.'
  } finally {
    confirmingBulk.value = null
  }
}

/**
 * Replay can be refused, and the refusal is a decision rather than a fault:
 * the server rejects one whose logical fire time is older than the job's
 * `dead_letter_replay_max_age`. Reporting the server's own words is the
 * difference between "replay failed" and knowing why.
 */
/**
 * The one refusal that is worth arguing with.
 *
 * `stale_replay` is a policy the job declares, not a fault — and the server's
 * own message ends with "Pass force:true to replay anyway". Holding the id
 * here is what turns that sentence into a button; any other refusal just gets
 * reported (issue #660).
 */
const forceable = ref<string | null>(null)

async function doReplay(id: string, force = false) {
  replayError.value = null
  forceable.value = null
  try {
    await replay.mutateAsync({ id, force })
    if (selectedId.value === id) selectedId.value = null
  } catch (caught) {
    const body =
      caught instanceof ApiError ? (caught.body as { message?: string; error?: string }) : undefined
    replayError.value = body?.message ?? (caught as Error).message ?? 'Replay was refused.'
    if (body?.error === 'stale_replay') forceable.value = id
  }
}

async function doDelete(id: string) {
  // Reuses the replay banner: one row can only be in one of these states, and
  // a second alert bar for the same list would be noise.
  replayError.value = null
  try {
    await remove.mutateAsync(id)
    if (selectedId.value === id) selectedId.value = null
  } catch (caught) {
    const body = caught instanceof ApiError ? (caught.body as { message?: string }) : undefined
    replayError.value = body?.message ?? (caught as Error).message ?? 'The server refused that.'
  }
}

const expiring = (row: DeadLetter) => Boolean(row.expires_at)
</script>

<template>
  <div class="flex h-full min-h-0 flex-col gap-4">
    <div class="flex flex-wrap items-center gap-3">
      <p class="text-sm text-muted">
        Runs that exhausted their retries. Each one is waiting for a decision.
      </p>

      <div class="ml-auto flex items-center gap-2">
        <!-- Only once something is picked. A destructive control sitting
             permanently beside a work queue is one an operator eventually
             stops reading. -->
        <template v-if="pickedRows.length">
          <span class="cq-num text-sm text-muted">{{ pickedRows.length }} selected</span>
          <UButton
            icon="i-lucide-trash-2"
            color="error"
            variant="subtle"
            size="sm"
            :loading="bulkDelete.isPending.value"
            @click="confirmingBulk = 'picked'"
          >
            Discard selected
          </UButton>
          <UButton
            color="neutral"
            variant="ghost"
            size="sm"
            @click="picked = new Set()"
          >
            Clear
          </UButton>
        </template>
        <UButton
          v-else-if="rows.length"
          icon="i-lucide-trash-2"
          color="neutral"
          variant="ghost"
          size="sm"
          @click="confirmingBulk = 'all'"
        >
          Discard all
        </UButton>
        <span class="cq-num text-sm text-muted">
          {{ total }} pending<template v-if="total > rows.length">, {{ rows.length }} shown</template>
        </span>
      </div>
    </div>

    <UAlert
      v-if="bulkNotice"
      color="info"
      variant="subtle"
      icon="i-lucide-info"
      :description="bulkNotice"
      role="status"
      close
      @update:open="bulkNotice = null"
    />

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
    >
      <template
        v-if="forceable"
        #actions
      >
        <UButton
          color="warning"
          variant="solid"
          size="xs"
          :loading="replay.isPending.value"
          @click="doReplay(forceable, true)"
        >
          Replay anyway
        </UButton>
      </template>
    </UAlert>

    <div class="flex min-h-0 flex-1 gap-4">
      <div class="cq-list min-w-0 flex-1">
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
              <th class="w-10 px-[var(--cq-cell-x)] py-[var(--cq-cell-y)]">
                <UCheckbox
                  :model-value="allPicked"
                  :indeterminate="pickedRows.length > 0 && !allPicked"
                  aria-label="Select every dead letter"
                  @update:model-value="toggleAll"
                />
              </th>
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
              <!-- `@click.stop`: picking a row for a bulk action is a
                   different intent from opening it, and the two share a row. -->
              <td
                class="px-[var(--cq-cell-x)]"
                @click.stop
              >
                <UCheckbox
                  :model-value="picked.has(row.id)"
                  :aria-label="`Select the dead letter for ${row.job_key}`"
                  @update:model-value="togglePick(row.id)"
                />
              </td>
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

      <ConfirmModal
        :open="confirmingBulk !== null"
        :title="confirmingBulk === 'all' ? 'Discard every dead letter?' : `Discard ${pickedRows.length} dead letter${pickedRows.length === 1 ? '' : 's'}?`"
        :description="
          confirmingBulk === 'all'
            ? `All ${total} of them go — the whole queue, not just the ${rows.length} on screen, and including any that arrive while this dialog is open. Discarding is not replaying — the work does not run.`
            : 'Discarding is not replaying — the work does not run. The runs stay in the history.'
        "
        confirm-label="Discard"
        :loading="bulkDelete.isPending.value"
        @update:open="(open: boolean) => { if (!open) confirmingBulk = null }"
        @confirm="runBulk"
      />

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
