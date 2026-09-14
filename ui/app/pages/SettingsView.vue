<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useIsAdmin } from '~/composables/useIsAdmin'

/**
 * Settings: you, the people, the machines, the log.
 *
 * Four views on one screen, addressed by path rather than by `?tab` — the
 * React tree used a query parameter, and the rest of this tree uses path
 * segments for exactly this shape (see the alerts screen). One idiom.
 *
 * Three of the four are admin surfaces. They are hidden rather than shown and
 * refused: a tab that always answers 403 teaches people to ignore errors.
 */
const route = useRoute()
const router = useRouter()
const isAdmin = useIsAdmin()

const view = computed<'profile' | 'people' | 'clients' | 'audit'>(() => {
  if (route.path.startsWith('/settings/people')) return 'people'
  if (route.path.startsWith('/settings/clients')) return 'clients'
  if (route.path.startsWith('/settings/audit')) return 'audit'
  return 'profile'
})

const VIEWS = computed(() => [
  { label: 'Profile', value: 'profile' as const },
  ...(isAdmin.value
    ? [
        { label: 'People', value: 'people' as const },
        { label: 'API clients', value: 'clients' as const },
        { label: 'Audit log', value: 'audit' as const },
      ]
    : []),
])

function switchTo(target: string) {
  void router.push(target === 'profile' ? '/settings' : `/settings/${target}`)
}

/**
 * A non-admin who deep-links into an admin view sees the profile instead of an
 * error. The server would refuse the request anyway; this keeps the refusal
 * from being the first thing they see.
 */
const effective = computed(() => (view.value !== 'profile' && !isAdmin.value ? 'profile' : view.value))
</script>

<template>
  <div class="flex h-full min-h-0 flex-col gap-4">
    <UTabs
      :model-value="effective"
      :items="VIEWS"
      :content="false"
      variant="link"
      size="sm"
      @update:model-value="(value: string | number) => switchTo(String(value))"
    />

    <!-- Profile, people and clients are documents: they scroll as a whole.
         The audit log is a list and owns its own scroll region. -->
    <div
      v-if="effective === 'audit'"
      class="min-h-0 flex-1"
    >
      <SettingsAudit class="h-full" />
    </div>
    <div
      v-else
      class="min-h-0 flex-1 overflow-y-auto"
    >
      <SettingsProfile v-if="effective === 'profile'" />
      <SettingsPeople v-else-if="effective === 'people'" />
      <SettingsClients v-else-if="effective === 'clients'" />
    </div>
  </div>
</template>
