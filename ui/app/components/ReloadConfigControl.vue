<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ApiError } from '~/api/client'
import { useReloadConfig } from '~/api/queries'
import type { ReloadFailure, ReloadSuccess } from '~/api/types'

/**
 * Re-read the Croniqfile from the dashboard.
 *
 * The endpoint has existed since #471 and the React dashboard never called it,
 * so without `--watch` an operator's only ways to apply a schedule edit were
 * SIGHUP and curl. This is that button.
 *
 * It is a *preview* first, because `?dry_run=true` makes that free: the server
 * validates the file, computes which jobs would be added, removed or changed,
 * and lists the boot-only settings this reload cannot apply — all without
 * touching anything. Applying blind when the server will happily tell you what
 * is about to happen would be a worse button than none.
 *
 * The reason the pending-restart list matters more than it looks: `server { }`,
 * `auth { }`, `alerts { }` and friends are read at boot only, so editing one and
 * reloading is a no-op that *reports success*. That was silent until #406 gave
 * it a name. A green tick here would re-create exactly the confusion the server
 * side went to the trouble of removing.
 */
const reload = useReloadConfig()

const open = ref(false)
const preview = ref<ReloadSuccess | null>(null)
const result = ref<ReloadSuccess | null>(null)
const failure = ref<ReloadFailure | null>(null)

/** Fresh state every time the popover opens; a stale diff is a lie. */
watch(open, (isOpen) => {
  preview.value = null
  result.value = null
  failure.value = null
  if (isOpen) void run(true)
})

async function run(dryRun: boolean) {
  failure.value = null
  try {
    const response = await reload.mutateAsync({ dryRun })
    if (dryRun) preview.value = response
    else result.value = response
  } catch (caught) {
    // 422 carries the parse error with a line and column. Surfacing that is
    // most of the value of validating before applying.
    const body = caught instanceof ApiError ? (caught.body as ReloadFailure | undefined) : undefined
    failure.value = body?.message
      ? body
      : { error: 'error', message: (caught as Error).message || 'Reload failed.' }
  }
}

const shown = computed(() => result.value ?? preview.value)
const diff = computed(() => shown.value?.diff)
const pending = computed(() => shown.value?.pending_restart ?? [])
const credentials = computed(() => shown.value?.credentials ?? [])

const noop = computed(
  () =>
    diff.value !== undefined &&
    diff.value.added.length === 0 &&
    diff.value.removed.length === 0 &&
    diff.value.changed.length === 0,
)

/** Nothing to apply *and* nothing a restart would fix. */
const nothingToDo = computed(() => noop.value && pending.value.length === 0)

const changes = computed(() => {
  if (!diff.value) return []
  return [
    { label: 'Added', keys: diff.value.added, color: 'success' as const },
    { label: 'Removed', keys: diff.value.removed, color: 'error' as const },
    { label: 'Changed', keys: diff.value.changed, color: 'info' as const },
  ].filter((group) => group.keys.length > 0)
})
</script>

<template>
  <UPopover v-model:open="open">
    <UButton
      color="neutral"
      variant="subtle"
      icon="i-lucide-refresh-cw"
      aria-label="Reload the Croniqfile"
      title="Reload the Croniqfile"
    />

    <template #content>
      <div class="w-96 p-4">
        <div class="mb-1 flex items-center justify-between">
          <span class="font-medium">Reload Croniqfile</span>
          <UBadge
            v-if="result"
            color="success"
            variant="subtle"
            size="sm"
          >
            applied
          </UBadge>
        </div>
        <p class="mb-4 text-sm text-muted">
          Re-reads jobs, calendars, triggers and policy flags. Server, auth,
          alerts and the other boot-only blocks need a restart.
        </p>

        <AppLoading
          v-if="reload.isPending.value && !shown"
          size="tight"
          label="Validating the Croniqfile"
        />

        <!-- A file that does not parse. Line and column come from the server;
             they are the difference between "reload failed" and a fix. -->
        <UAlert
          v-else-if="failure"
          color="error"
          variant="subtle"
          icon="i-lucide-file-x"
          :title="failure.line ? `Line ${failure.line}${failure.column ? `, column ${failure.column}` : ''}` : 'Could not reload'"
          :description="failure.message"
          role="alert"
          class="mb-3"
        />

        <template v-else-if="shown">
          <AppEmpty
            v-if="nothingToDo"
            size="tight"
            icon="i-lucide-check"
            title="Already up to date"
            description="The file on disk matches what is running."
          />

          <div
            v-else
            class="flex flex-col gap-3"
          >
            <div v-if="changes.length">
              <p class="cq-label mb-1.5">
                {{ result ? 'Applied' : 'Would apply' }}
              </p>
              <ul class="flex flex-col gap-1.5">
                <li
                  v-for="group in changes"
                  :key="group.label"
                  class="flex gap-2 text-sm"
                >
                  <UBadge
                    :color="group.color"
                    variant="subtle"
                    size="sm"
                    class="shrink-0"
                  >
                    {{ group.label }} {{ group.keys.length }}
                  </UBadge>
                  <span class="min-w-0 font-mono text-xs break-all text-muted">
                    {{ group.keys.join(', ') }}
                  </span>
                </li>
              </ul>
              <p class="mt-1.5 text-xs text-dimmed">
                {{ diff?.total }} jobs after reload
              </p>
            </div>

            <!-- The honest part. A reload reports success even when half the
                 edit did not land, and this is what says so. -->
            <div v-if="pending.length">
              <p class="cq-label mb-1.5 text-warning">
                Needs a restart — {{ pending.length }}
              </p>
              <ul class="flex flex-col gap-1 text-xs">
                <li
                  v-for="item in pending"
                  :key="item.setting"
                  class="flex flex-wrap items-baseline gap-x-2 font-mono"
                >
                  <span class="text-toned">{{ item.setting }}</span>
                  <span class="text-dimmed">
                    {{ item.running ?? 'unset' }} → {{ item.pending ?? 'removed' }}
                  </span>
                </li>
              </ul>
              <p class="mt-1.5 text-xs text-muted">
                Boot-only. Restart the process, or recreate the container.
              </p>
            </div>

            <div v-if="credentials.length">
              <p class="cq-label mb-1.5">
                API clients
              </p>
              <ul class="flex flex-col gap-1 text-xs">
                <li
                  v-for="item in credentials"
                  :key="item.client"
                  class="flex gap-2"
                >
                  <span class="font-mono text-toned">{{ item.client }}</span>
                  <span class="text-dimmed">{{ item.action.replace('_', ' ') }}</span>
                </li>
              </ul>
            </div>

            <UAlert
              v-if="shown.credentials_error"
              color="warning"
              variant="subtle"
              :description="shown.credentials_error"
              role="status"
            />
          </div>
        </template>

        <div class="mt-4 flex items-center gap-2">
          <UButton
            v-if="!result"
            :loading="reload.isPending.value"
            :disabled="!preview || nothingToDo"
            icon="i-lucide-check"
            @click="run(false)"
          >
            Apply
          </UButton>
          <UButton
            color="neutral"
            variant="ghost"
            :loading="reload.isPending.value"
            @click="run(true)"
          >
            {{ result ? 'Check again' : 'Re-check' }}
          </UButton>
        </div>
      </div>
    </template>
  </UPopover>
</template>
