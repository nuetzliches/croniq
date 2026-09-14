<script setup lang="ts">
import { computed, ref } from 'vue'
import { useVersion } from '~/api/queries'
import { UI_VERSION, skewDismissalKey, versionSkew } from '~/lib/build-version'

/**
 * "This dashboard and this server were not released together."
 *
 * Only possible in the split topology (`croniq-ui` + `croniq-server` as
 * separate images, #587/#598); the combined image is immune by construction.
 * See `~/lib/build-version` for why the comparison is shaped the way it is.
 *
 * A warning, not a wall. A mismatched pair is usually still mostly functional,
 * and locking an operator out of their own dashboard over a version string is
 * worse than the skew it would be protecting them from.
 */
const { data: server } = useVersion()

const skew = computed(() => versionSkew(UI_VERSION, server.value?.version))

const dismissed = ref<string | null>(readDismissal())

function readDismissal(): string | null {
  try {
    return localStorage.getItem('croniq_version_skew_dismissed')
  } catch {
    // Private mode, or storage disabled. Then the warning simply reappears on
    // the next load, which is the safe direction to fail in.
    return null
  }
}

function dismiss() {
  if (!skew.value) return
  const key = skewDismissalKey(skew.value)
  dismissed.value = key
  try {
    localStorage.setItem('croniq_version_skew_dismissed', key)
  } catch {
    // Same — dismissing still works for this page view.
  }
}

const show = computed(() => {
  const current = skew.value
  if (!current) return false
  return dismissed.value !== skewDismissalKey(current)
})
</script>

<template>
  <!-- `role="status"`, not `alert`: this is a standing condition that was
       already true when the page opened, not something that just happened.
       An alert interrupts a screen reader mid-sentence. -->
  <UAlert
    v-if="show && skew"
    color="warning"
    variant="subtle"
    icon="i-lucide-git-compare-arrows"
    role="status"
    title="Dashboard and server versions do not match"
    :close="{ 'aria-label': 'Dismiss the version mismatch warning' }"
    @update:open="dismiss"
  >
    <template #description>
      This dashboard is <strong>v{{ skew.ui }}</strong> and the server is
      <strong>v{{ skew.server }}</strong>. They are published together under
      one tag, so a pair this far apart was pinned by hand. Screens may call
      endpoints the other half does not have, which shows up as a scattered
      404 rather than as anything naming the cause — match the two image tags.
    </template>
  </UAlert>
</template>
