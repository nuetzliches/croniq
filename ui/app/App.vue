<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { useVersion } from '~/api/queries'
import { bootstrap } from '~/api/session'
import { observeServerVersion } from '~/lib/server-update'
import { useAuthStore } from '~/stores/auth'

const auth = useAuthStore()
const route = useRoute()

// Redeem the refresh cookie before the first route renders anything that needs
// a session. The router guard waits for the store to leave `unknown`, so this
// is what unblocks it. See ADR-0001 for why a reload starts with no token.
// `bootstrap` never rejects and always leaves `unknown` (review 2026-10-24, U-2).
onMounted(() => {
  void bootstrap()
})

// While the guard waits, nothing is routed yet and `<RouterView>` renders
// nothing. That used to be the whole page whenever the refresh hung behind a
// proxy: blank, with no hint of what it was waiting for (U-2). Shown after a
// short delay so an ordinary reload does not flash it, and with a hint once the
// wait stops being ordinary.
const showWaiting = ref(false)
const waitingLong = ref(false)
const timers = [
  setTimeout(() => (showWaiting.value = true), 400),
  setTimeout(() => (waitingLong.value = true), 10_000),
]
onBeforeUnmount(() => timers.forEach(clearTimeout))

// Here rather than in the shell: the root is mounted on every route, signed in
// or not, so the first answer — the baseline an upgrade is measured against —
// is the one this page load actually started with.
const { data: version } = useVersion()
watch(version, observeServerVersion, { immediate: true })
</script>

<template>
  <UApp>
    <div
      v-if="showWaiting && auth.status === 'unknown' && route.matched.length === 0"
      class="flex min-h-svh flex-col items-center justify-center gap-3 p-6 text-center"
      role="status"
      data-testid="session-pending"
    >
      <UIcon
        name="i-lucide-loader-circle"
        class="size-6 animate-spin text-muted"
        aria-hidden="true"
      />
      <p class="text-sm text-muted">
        Connecting to the server…
      </p>
      <p
        v-if="waitingLong"
        class="max-w-md text-sm text-muted"
      >
        This is taking longer than it should. Check that the Croniq server is
        reachable and that a proxy in front of it forwards <code>/v1/</code>.
      </p>
    </div>
    <RouterView />
  </UApp>
</template>
