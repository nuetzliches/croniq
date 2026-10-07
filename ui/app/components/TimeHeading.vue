<script setup lang="ts">
import { useTimeDisplay, useTimeDisplayShortcut } from '~/composables/useTimeDisplay'

/**
 * The heading of a table's time column: its label, the zone once clock times
 * are on, and the switch between the two (also `t`, anywhere on the screen).
 *
 * The zone is named here once rather than as an abbreviation in every row,
 * which would be wider than the time and, across a DST change, different from
 * row to row — the way the run's log panel names it beside "Logs". "3 min ago"
 * is the same in every zone, so relative columns say nothing.
 *
 * The switch is its own small control rather than a click on the label, which
 * is where sorting would go. Content only: the `<th>`, its alignment and its
 * `v-if` stay with the table.
 */
defineProps<{ label: string }>()

const { clock, timeZone, toggle } = useTimeDisplay()
useTimeDisplayShortcut()
</script>

<template>
  <div class="inline-flex items-center gap-1">
    {{ label }}
    <span
      v-if="clock"
      v-tooltip="'Times are shown in your browser\'s time zone.'"
      class="font-normal normal-case text-dimmed"
      data-testid="time-heading-zone"
    >{{ timeZone }}</span>
    <UButton
      v-tooltip="clock ? 'Show how long ago (t)' : 'Show the time of day (t)'"
      variant="ghost"
      color="neutral"
      size="xs"
      class="-my-1"
      :icon="clock ? 'i-lucide-clock' : 'i-lucide-history'"
      :aria-pressed="clock"
      :aria-label="clock ? 'Show how long ago' : 'Show the time of day'"
      data-testid="time-heading-toggle"
      @click="toggle()"
    />
  </div>
</template>
