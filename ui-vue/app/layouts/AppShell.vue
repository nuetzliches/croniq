<script setup lang="ts">
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import { useCurrentUser, useDeadLetterCount } from '~/api/queries'
import { logout } from '~/api/session'
import { useUiStore } from '~/stores/ui'
import { NAV_SECTIONS } from '~/router/nav'

const route = useRoute()
const ui = useUiStore()
const { data: me } = useCurrentUser()
const deadLetters = useDeadLetterCount()

/**
 * The console tails the server's whole tracing stream, so
 * `GET /v1/events/stream` needs the `admin` scope. Hide it for known
 * non-admins — but only when the role is actually known. While `me` is still
 * loading the caller may well be an admin, and an entry that appears a moment
 * later is less confusing than one that answers 403.
 */
const isAdmin = computed(() => (me.value ? me.value.role === 'admin' : true))

const sections = computed(() =>
  NAV_SECTIONS.map((section) => ({
    ...section,
    items: section.items.filter((item) => !item.adminOnly || isAdmin.value),
  })).filter((section) => section.items.length > 0),
)

const currentTitle = computed(() => {
  for (const section of NAV_SECTIONS) {
    const hit = section.items.find((item) => item.to === route.path)
    if (hit) return hit.label
  }
  return route.meta.title ?? ''
})

async function signOut() {
  await logout()
  window.location.assign('/login')
}
</script>

<template>
  <div class="flex min-h-screen bg-default">
    <!--
      `<nav>` with an accessible name, because a page with several landmarks of
      the same type is unnavigable without one. The Playwright suite selects on
      exactly this name, so it is a contract as well as an affordance.
    -->
    <nav
      aria-label="Main navigation"
      class="flex shrink-0 flex-col border-r border-default bg-elevated transition-[width]"
      :class="ui.sidebarCollapsed ? 'w-16' : 'w-60'"
    >
      <div class="flex h-14 items-center gap-2 px-4">
        <UIcon
          name="i-lucide-timer"
          class="size-5 shrink-0 text-primary"
        />
        <span
          v-if="!ui.sidebarCollapsed"
          class="font-semibold"
        >Croniq</span>
      </div>

      <div class="flex-1 overflow-y-auto px-2 pb-4">
        <template
          v-for="section in sections"
          :key="section.label"
        >
          <p
            v-if="!ui.sidebarCollapsed"
            class="px-2 pt-4 pb-1 text-[11px] font-medium tracking-wide text-muted uppercase"
          >
            {{ section.label }}
          </p>
          <ul class="flex flex-col gap-0.5">
            <li
              v-for="item in section.items"
              :key="item.to"
            >
              <RouterLink
                :to="item.to"
                :title="ui.sidebarCollapsed ? item.label : undefined"
                class="flex items-center gap-2.5 rounded-md px-2 py-1.5 text-sm text-toned hover:bg-accented hover:text-highlighted"
                active-class="bg-accented text-highlighted"
              >
                <UIcon
                  :name="item.icon"
                  class="size-4 shrink-0"
                />
                <span
                  v-if="!ui.sidebarCollapsed"
                  class="flex-1 truncate"
                >{{ item.label }}</span>
                <UBadge
                  v-if="item.to === '/dead-letters' && deadLetters > 0"
                  color="error"
                  variant="subtle"
                  size="sm"
                >
                  {{ deadLetters }}
                </UBadge>
              </RouterLink>
            </li>
          </ul>
        </template>
      </div>
    </nav>

    <div class="flex min-w-0 flex-1 flex-col">
      <header class="flex h-14 shrink-0 items-center gap-3 border-b border-default px-4">
        <UButton
          icon="i-lucide-panel-left"
          color="neutral"
          variant="ghost"
          aria-label="Toggle sidebar"
          @click="ui.toggleSidebar()"
        />
        <h1 class="text-sm font-medium">
          {{ currentTitle }}
        </h1>

        <div class="ml-auto flex items-center gap-2">
          <!--
            A real listbox rather than three bare buttons in a div with
            role="menu" — which is what the React tree does and what makes its
            user menu unenumerable by assistive tech (#595).
          -->
          <USelect
            v-model="ui.theme"
            :items="[
              { label: 'System', value: 'system' },
              { label: 'Light', value: 'light' },
              { label: 'Dark', value: 'dark' },
            ]"
            aria-label="Colour theme"
            size="sm"
            class="w-28"
          />
          <UDropdownMenu
            :items="[
              [
                { label: 'Profile & account', icon: 'i-lucide-user', to: '/settings?tab=profile' },
                { label: 'Sign out', icon: 'i-lucide-log-out', onSelect: signOut },
              ],
            ]"
          >
            <UButton
              color="neutral"
              variant="ghost"
              trailing-icon="i-lucide-chevron-down"
              :aria-label="me ? `Account menu for ${me.username}` : 'Account menu'"
            >
              {{ me?.display_name ?? me?.username ?? '…' }}
            </UButton>
          </UDropdownMenu>
        </div>
      </header>

      <main class="min-w-0 flex-1 overflow-y-auto p-6">
        <RouterView />
      </main>
    </div>
  </div>
</template>
