<script setup lang="ts">
import { serverUpdate } from '~/lib/server-update'

/**
 * "Croniq was upgraded while this tab was open."
 *
 * The next page change reloads by itself (the router guard); this is for the
 * operator who stays on one screen and keeps clicking actions, which goes on
 * talking to the new server from the old dashboard. Not dismissible: it is
 * cured by one click, and until then it is true.
 */
function reload() {
  window.location.reload()
}
</script>

<template>
  <!-- role="status": a standing condition, not an interruption. -->
  <UAlert
    v-if="serverUpdate"
    color="info"
    variant="subtle"
    icon="i-lucide-refresh-cw"
    role="status"
    title="Croniq has been updated"
    :actions="[{ label: 'Reload now', color: 'info', variant: 'solid', onClick: reload }]"
  >
    <template #description>
      The server now runs <strong>v{{ serverUpdate.to }}</strong>; this page was
      loaded from <strong>v{{ serverUpdate.from }}</strong>. It reloads by itself
      on your next page change — or reload now to use the new dashboard here.
    </template>
  </UAlert>
</template>
