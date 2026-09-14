<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useMaintenance, useSetMaintenance } from '~/api/queries'

/**
 * Turn maintenance on or off, from the topbar.
 *
 * The placement is carried over from the React tree and it is the right one:
 * this is an operational action you reach in a hurry — something is wrong,
 * stop dispatching — not a setting you configure once. Burying it in Settings
 * would put two navigations between an operator and the brake.
 *
 * Admin-only, enforced by the server; the trigger is simply not rendered for
 * anyone else so it is not a button that only ever answers 403.
 */
const { data } = useMaintenance()
const setMaintenance = useSetMaintenance()

const open = ref(false)
const manual = ref(false)
const start = ref('')
const end = ref('')
const note = ref('')

const active = computed(() => data.value?.active ?? false)

/**
 * `<input type="datetime-local">` speaks local wall-clock without a zone; the
 * API speaks UTC ISO. These two conversions are the whole reason the fields
 * are not bound directly, and getting either backwards silently shifts a
 * maintenance window by the machine's offset.
 */
function isoToLocalInput(iso: string | null | undefined): string {
  if (!iso) return ''
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return ''
  const offsetMs = date.getTimezoneOffset() * 60_000
  return new Date(date.getTime() - offsetMs).toISOString().slice(0, 16)
}

function localInputToIso(value: string): string | null {
  if (!value) return null
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? null : date.toISOString()
}

/**
 * Re-seed from the server every time the popover opens, so an admin edits the
 * live values rather than whatever was on screen the last time. Carried over
 * from the React version, which is explicit about the same thing.
 */
watch(open, (isOpen) => {
  if (!isOpen) return
  manual.value = data.value?.manual_active ?? false
  start.value = isoToLocalInput(data.value?.window_start)
  end.value = isoToLocalInput(data.value?.window_end)
  note.value = data.value?.note ?? ''
})

async function save() {
  await setMaintenance.mutateAsync({
    manual_active: manual.value,
    window_start: localInputToIso(start.value),
    window_end: localInputToIso(end.value),
    note: note.value.trim() || null,
  })
  open.value = false
}

async function clearAll() {
  await setMaintenance.mutateAsync({
    manual_active: false,
    window_start: null,
    window_end: null,
    note: null,
  })
  open.value = false
}
</script>

<template>
  <UPopover v-model:open="open">
    <UButton
      color="neutral"
      :variant="active ? 'soft' : 'subtle'"
      icon="i-lucide-wrench"
      :class="active && 'text-warning'"
      :aria-label="active ? 'Maintenance mode is on' : 'Maintenance mode'"
      :title="active ? 'Maintenance mode is on' : 'Maintenance mode'"
    />

    <template #content>
      <div class="w-80 p-4">
        <div class="mb-3 flex items-center justify-between">
          <span class="font-medium">Maintenance mode</span>
          <UBadge
            :color="active ? 'warning' : 'neutral'"
            variant="subtle"
            size="sm"
          >
            {{ active ? 'active' : 'off' }}
          </UBadge>
        </div>

        <p class="mb-4 text-sm text-muted">
          Pauses job dispatch. Running jobs finish; scheduled and queued work
          resumes when maintenance ends.
        </p>

        <div class="flex flex-col gap-3">
          <UCheckbox
            v-model="manual"
            label="On until I turn it off"
            description="Independent of the window below."
          />

          <UFormField
            label="Window start"
            name="window-start"
            help="Local time. Leave empty for no scheduled window."
          >
            <UInput
              v-model="start"
              type="datetime-local"
              class="w-full"
            />
          </UFormField>

          <UFormField
            label="Window end"
            name="window-end"
          >
            <UInput
              v-model="end"
              type="datetime-local"
              class="w-full"
            />
          </UFormField>

          <UFormField
            label="Note"
            name="note"
            help="Shown in the banner to everyone."
          >
            <UInput
              v-model="note"
              placeholder="Migrating the database…"
              class="w-full"
            />
          </UFormField>
        </div>

        <div class="mt-4 flex items-center gap-2">
          <UButton
            :loading="setMaintenance.isPending.value"
            @click="save"
          >
            Save
          </UButton>
          <UButton
            color="neutral"
            variant="ghost"
            :disabled="!active && !data?.window_end"
            @click="clearAll"
          >
            Turn off
          </UButton>
        </div>
      </div>
    </template>
  </UPopover>
</template>
