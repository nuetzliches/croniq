<script setup lang="ts">
import { onMounted } from 'vue'
import { bootstrap } from '~/api/session'

// Redeem the refresh cookie before the first route renders anything that needs
// a session. The router guard waits for the store to leave `unknown`, so this
// is what unblocks it. See ADR-0001 for why a reload starts with no token.
onMounted(() => {
  void bootstrap()
})
</script>

<template>
  <UApp>
    <RouterView />
  </UApp>
</template>
