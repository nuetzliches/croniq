<script setup lang="ts">
import { onMounted, watch } from 'vue'
import { useVersion } from '~/api/queries'
import { bootstrap } from '~/api/session'
import { observeServerVersion } from '~/lib/server-update'

// Redeem the refresh cookie before the first route renders anything that needs
// a session. The router guard waits for the store to leave `unknown`, so this
// is what unblocks it. See ADR-0001 for why a reload starts with no token.
onMounted(() => {
  void bootstrap()
})

// Here rather than in the shell: the root is mounted on every route, signed in
// or not, so the first answer — the baseline an upgrade is measured against —
// is the one this page load actually started with.
const { data: version } = useVersion()
watch(version, observeServerVersion, { immediate: true })
</script>

<template>
  <UApp>
    <RouterView />
  </UApp>
</template>
