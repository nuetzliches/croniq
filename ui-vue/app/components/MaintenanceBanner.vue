<script setup lang="ts">
import { computed } from 'vue'
import { useMaintenance } from '~/api/queries'

/**
 * Maintenance mode, announced wherever you are.
 *
 * It pauses job dispatch, so a dashboard that looks normal while nothing fires
 * is actively misleading — which is why this sits in the shell rather than on
 * one screen. Carried over from the React tree's `MaintenanceBanner`.
 *
 * Two things it does that the original does not.
 *
 * It distinguishes *why* it is on. `active` is true both for the manual toggle
 * and for a scheduled window, and those need different things from the reader:
 * a manual one is waiting for somebody to turn it off, a windowed one ends by
 * itself. The original shows the same sentence for both.
 *
 * And it says when. The original appends `toLocaleString()` of the window end,
 * which is precise and hard to act on; "ends in about 40 minutes" is what an
 * operator is actually asking. The absolute time stays in the `title` for
 * anyone who needs it.
 */
const { data } = useMaintenance()

const until = computed(() => {
  const raw = data.value?.window_end
  if (!raw) return null
  const date = new Date(raw)
  return Number.isNaN(date.getTime()) ? null : date
})

const relative = computed(() => {
  if (!until.value) return null
  const minutes = Math.round((until.value.getTime() - Date.now()) / 60_000)
  if (minutes <= 0) return 'ending now'
  if (minutes < 60) return `ends in about ${minutes} minute${minutes === 1 ? '' : 's'}`
  const hours = Math.round(minutes / 60)
  if (hours < 24) return `ends in about ${hours} hour${hours === 1 ? '' : 's'}`
  const days = Math.round(hours / 24)
  return `ends in about ${days} day${days === 1 ? '' : 's'}`
})

const description = computed(() => {
  const note = data.value?.note?.trim()
  if (note) return note
  return 'Job dispatch is paused. Running jobs finish; scheduled and queued work resumes when maintenance ends.'
})

const title = computed(() =>
  until.value
    ? 'Maintenance window is active'
    : 'Maintenance mode is on',
)
</script>

<template>
  <!-- role="status" rather than "alert": this is a standing condition, not an
       event, and an alert interrupts a screen reader mid-sentence every time
       the poll re-renders it. -->
  <UAlert
    v-if="data?.active"
    color="warning"
    variant="subtle"
    icon="i-lucide-wrench"
    role="status"
    :title="title"
  >
    <template #description>
      {{ description }}
      <span
        v-if="relative"
        :title="until?.toLocaleString()"
        class="whitespace-nowrap"
      >— {{ relative }}.</span>
      <span v-else-if="data?.manual_active">— until someone turns it off.</span>
    </template>
  </UAlert>
</template>
