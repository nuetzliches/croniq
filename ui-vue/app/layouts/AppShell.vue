<script setup lang="ts">
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import { useCurrentUser, useDeadLetterCount } from '~/api/queries'
import { useHealth, useVersion } from '~/api/queries'
import { logout } from '~/api/session'
import { useUiStore } from '~/stores/ui'
import { NAV_SECTIONS } from '~/router/nav'

/**
 * The frame: sidebar, topbar and content as cards floating on the ground
 * defined in assets/css/main.css.
 *
 * That shape is carried over from the shipping dashboard, where it is most of
 * the reason the product does not read as a generic admin panel
 * (docs/ui-visual-design.md). What is *not* carried over is the full-bleed
 * flatness of the first Vue shell, which was Nuxt UI's default and looked it.
 */
const route = useRoute()
const ui = useUiStore()
const { data: me } = useCurrentUser()
const { data: health } = useHealth()
const { data: version } = useVersion()
const deadLetters = useDeadLetterCount()

/**
 * The console tails the server's whole tracing stream, so
 * `GET /v1/events/stream` needs the `admin` scope. Hide it for known
 * non-admins — but only when the role is actually known. While `me` is still
 * loading the caller may well be one, and an entry appearing a moment later
 * beats one that answers 403.
 */
const isAdmin = computed(() => (me.value ? me.value.role === 'admin' : true))

/** What to call the signed-in user. */
const displayName = computed(() => me.value?.display_name ?? me.value?.username ?? '…')

/**
 * The role, as a second line — but only when it says something the first line
 * does not. The seeded demo account is a user named `admin` with the role
 * `admin`, and stacking those reads as a rendering bug rather than as
 * information.
 */
const roleLine = computed(() => {
  const role = me.value?.role
  if (!role) return null
  return role.toLowerCase() === displayName.value.toLowerCase() ? null : role
})

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
  return String(route.meta.title ?? '')
})

/**
 * One dot for "is the server answering". The shipping dashboard has the same
 * thing, and it earns its place on a scheduler: the dashboard can look
 * perfectly healthy while the process behind it has stopped.
 */
const live = computed(() => health.value !== undefined)

async function signOut() {
  await logout()
  window.location.assign('/login')
}
</script>

<template>
  <div class="flex min-h-screen gap-3 p-3">
    <!--
      `<nav>` with an accessible name: a page with several landmarks of one
      type is unnavigable without them, and the Playwright suite selects on
      exactly this name, so it is a contract as well as an affordance.
    -->
    <nav
      aria-label="Main navigation"
      class="flex shrink-0 flex-col rounded-xl border border-default bg-default shadow-sm transition-[width] duration-200"
      :class="ui.sidebarCollapsed ? 'w-14' : 'w-56'"
    >
      <div class="flex h-14 items-center gap-2.5 px-[1.125rem]">
        <BrandMark
          :size="22"
          chip
          class="shrink-0"
        />
        <span
          v-if="!ui.sidebarCollapsed"
          class="font-semibold tracking-tight"
        >Croniq</span>
      </div>

      <div class="flex-1 overflow-y-auto px-2 pb-3">
        <template
          v-for="section in sections"
          :key="section.label"
        >
          <!--
            Fixed height in both states. A heading when there is room for one,
            a rule when there is not — but the same 2.25rem either way, so
            nothing below shifts when the sidebar collapses.
          -->
          <div class="flex h-9 items-end px-2.5 pb-1.5">
            <p
              v-if="!ui.sidebarCollapsed"
              class="cq-label"
            >
              {{ section.label }}
            </p>
            <div
              v-else
              class="mb-1 w-full border-t border-default"
              aria-hidden="true"
            />
          </div>
          <ul class="flex flex-col gap-0.5">
            <li
              v-for="item in section.items"
              :key="item.to"
            >
              <RouterLink
                :to="item.to"
                :title="ui.sidebarCollapsed ? item.label : undefined"
                class="group relative flex h-8 items-center gap-2.5 rounded-lg px-2.5 text-sm text-toned transition-colors hover:bg-elevated hover:text-highlighted"
                active-class="bg-elevated text-highlighted font-medium"
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
                  :class="ui.sidebarCollapsed && 'absolute top-0.5 right-1.5 px-1'"
                >
                  {{ deadLetters }}
                </UBadge>
              </RouterLink>
            </li>
          </ul>
        </template>
      </div>

      <!-- Who you are, at the bottom, where the shipping dashboard keeps it. -->
      <div class="border-t border-default p-2">
        <UDropdownMenu
          :items="[
            [
              { label: 'Profile & account', icon: 'i-lucide-user', to: '/settings?tab=profile' },
              { label: 'API keys & clients', icon: 'i-lucide-key', to: '/settings?tab=clients' },
            ],
            [{ label: 'Sign out', icon: 'i-lucide-log-out', color: 'error', onSelect: signOut }],
          ]"
        >
          <UButton
            color="neutral"
            variant="ghost"
            block
            class="h-11 justify-start gap-2.5 px-1.5"
            :aria-label="me ? `Account menu for ${me.username}` : 'Account menu'"
          >
            <UAvatar
              :alt="me?.username ?? '?'"
              size="xs"
            />
            <span
              v-if="!ui.sidebarCollapsed"
              class="flex min-w-0 flex-1 flex-col items-start text-left leading-tight"
            >
              <span class="w-full truncate text-sm font-medium">{{ displayName }}</span>
              <span
                v-if="roleLine"
                class="w-full truncate text-xs text-muted"
              >{{ roleLine }}</span>
            </span>
            <UIcon
              v-if="!ui.sidebarCollapsed"
              name="i-lucide-chevron-up"
              class="size-4 shrink-0 text-dimmed"
            />
          </UButton>
        </UDropdownMenu>
      </div>
    </nav>

    <div class="flex min-w-0 flex-1 flex-col gap-3">
      <header
        class="flex h-14 shrink-0 items-center gap-3 rounded-xl border border-default bg-default px-3 shadow-sm"
      >
        <UButton
          icon="i-lucide-panel-left"
          color="neutral"
          variant="subtle"
          aria-label="Toggle sidebar"
          @click="ui.toggleSidebar()"
        />
        <!-- Breadcrumb rather than a bare title: on detail routes the parent
             is the thing you most often want to get back to. -->
        <nav
          aria-label="Breadcrumb"
          class="flex min-w-0 items-center gap-1.5 text-sm"
        >
          <span class="text-muted">Croniq</span>
          <span
            class="text-dimmed"
            aria-hidden="true"
          >/</span>
          <span class="truncate font-medium text-highlighted">{{ currentTitle }}</span>
        </nav>

        <div class="ml-auto flex items-center gap-1.5">
          <UBadge
            v-if="version?.version"
            color="neutral"
            variant="subtle"
            class="h-8 px-2.5 font-mono"
          >
            v{{ version.version }}
          </UBadge>
          <!-- aria-live: an operator who cannot see the dot still needs to
               learn that the server stopped answering. -->
          <span
            class="flex h-8 items-center gap-2 rounded-md px-2.5 text-sm"
            :class="live ? 'text-success' : 'text-error'"
            role="status"
            aria-live="polite"
          >
            <span
              class="size-2 rounded-full"
              :class="live ? 'bg-success' : 'bg-error'"
              aria-hidden="true"
            />
            {{ live ? 'live' : 'offline' }}
          </span>
          <!-- Admin-only, and not rendered otherwise: a button that can only
               ever answer 403 is worse than no button. -->
          <ReloadConfigControl v-if="me && isAdmin" />
          <MaintenanceControl v-if="me && isAdmin" />
          <UDropdownMenu
            :items="[
              [
                {
                  label: 'System',
                  icon: 'i-lucide-monitor',
                  type: 'checkbox' as const,
                  checked: ui.theme === 'system',
                  onSelect: () => (ui.theme = 'system'),
                },
                {
                  label: 'Light',
                  icon: 'i-lucide-sun',
                  type: 'checkbox' as const,
                  checked: ui.theme === 'light',
                  onSelect: () => (ui.theme = 'light'),
                },
                {
                  label: 'Dark',
                  icon: 'i-lucide-moon',
                  type: 'checkbox' as const,
                  checked: ui.theme === 'dark',
                  onSelect: () => (ui.theme = 'dark'),
                },
              ],
            ]"
          >
            <UButton
              color="neutral"
              variant="subtle"
              :icon="
                ui.theme === 'light'
                  ? 'i-lucide-sun'
                  : ui.theme === 'dark'
                    ? 'i-lucide-moon'
                    : 'i-lucide-monitor'
              "
              :aria-label="`Colour theme: ${ui.theme}`"
            />
          </UDropdownMenu>
        </div>
      </header>

      <main class="min-w-0 flex-1 overflow-y-auto rounded-xl border border-default bg-default p-5 shadow-sm">
        <!-- Above the routed view, not inside it: maintenance pauses dispatch
             everywhere, so it has to be visible wherever you happen to be. -->
        <MaintenanceBanner class="mb-5" />
        <RouterView />
      </main>
    </div>
  </div>
</template>
