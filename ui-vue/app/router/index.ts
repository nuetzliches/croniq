import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router'
import { watch } from 'vue'
import { useAuthStore } from '~/stores/auth'

/**
 * Routes follow the screen set agreed in `docs/ui-screen-inventory.md`.
 *
 * Every screen in that set is now built. Until this point unbuilt routes
 * rendered a `NotBuiltYet` placeholder naming the build step they belonged to,
 * so the shell stayed walkable and nobody mistook a gap for a finished screen.
 * The placeholder is gone with the last gap; a route that does not exist is a
 * bug now, not a to-do.
 */
const routes: RouteRecordRaw[] = [
  {
    path: '/login',
    name: 'login',
    component: () => import('~/pages/LoginView.vue'),
    meta: { public: true, title: 'Sign in' },
  },
  {
    path: '/',
    component: () => import('~/layouts/AppShell.vue'),
    children: [
      {
        path: '',
        name: 'dashboard',
        component: () => import('~/pages/DashboardView.vue'),
        meta: { title: 'Dashboard' },
      },
      {
        path: 'executions',
        name: 'runs',
        component: () => import('~/pages/RunsView.vue'),
        meta: { title: 'Runs' },
      },
      {
        // Same component: the detail opens beside the list rather than
        // replacing it, so the list keeps its scroll and its filters.
        path: 'executions/:id',
        name: 'run',
        component: () => import('~/pages/RunsView.vue'),
        meta: { title: 'Runs' },
      },
      {
        path: 'runners',
        name: 'runners',
        component: () => import('~/pages/RunnersView.vue'),
        meta: { title: 'Runners' },
      },
      {
        path: 'dead-letters',
        name: 'dead-letters',
        component: () => import('~/pages/DeadLettersView.vue'),
        meta: { title: 'Dead Letters' },
      },
      {
        path: 'jobs',
        name: 'jobs',
        component: () => import('~/pages/JobsView.vue'),
        meta: { title: 'Jobs' },
      },
      {
        // Same component as the list, like /executions/:id — the detail opens
        // beside the list rather than replacing it.
        path: 'jobs/:jobKey',
        name: 'job',
        component: () => import('~/pages/JobsView.vue'),
        meta: { title: 'Job' },
      },
      {
        path: 'calendars',
        name: 'calendars',
        component: () => import('~/pages/CalendarsView.vue'),
        meta: { title: 'Calendars' },
      },
      {
        path: 'calendars/:calendarId',
        name: 'calendar',
        component: () => import('~/pages/CalendarsView.vue'),
        meta: { title: 'Calendar' },
      },
      {
        // One screen, three views, all addressable — see AlertsView.vue for
        // why the delivery log did not become a separate screen.
        path: 'alerts',
        name: 'alerts',
        component: () => import('~/pages/AlertsView.vue'),
        meta: { title: 'Alerts' },
      },
      {
        path: 'alerts/rules/:ruleName',
        name: 'alert-rule',
        component: () => import('~/pages/AlertsView.vue'),
        meta: { title: 'Alert rule' },
      },
      {
        path: 'alerts/channels',
        name: 'alert-channels',
        component: () => import('~/pages/AlertsView.vue'),
        meta: { title: 'Alert channels' },
      },
      {
        path: 'alerts/deliveries',
        name: 'alert-deliveries',
        component: () => import('~/pages/AlertsView.vue'),
        meta: { title: 'Alert deliveries' },
      },
      {
        path: 'console',
        name: 'console',
        component: () => import('~/pages/ConsoleView.vue'),
        meta: { title: 'Console' },
      },
      {
        path: 'settings',
        name: 'settings',
        component: () => import('~/pages/SettingsView.vue'),
        meta: { title: 'Settings' },
      },
      {
        // Path segments rather than `?tab=`, matching the alerts screen. One
        // idiom for "several views of one screen" across the tree.
        path: 'settings/:section(people|clients|audit)',
        name: 'settings-section',
        component: () => import('~/pages/SettingsView.vue'),
        meta: { title: 'Settings' },
      },
      { path: 'scaffold', name: 'scaffold', component: () => import('~/pages/ScaffoldCheck.vue'), meta: { title: 'Scaffold check' } },
    ],
  },
  {
    path: '/:pathMatch(.*)*',
    name: 'not-found',
    component: () => import('~/pages/NotFoundView.vue'),
    meta: { public: true, title: 'Not found' },
  },
]

export const router = createRouter({
  history: createWebHistory(),
  routes,
})

/**
 * Send anonymous visitors to the login page.
 *
 * `status === 'unknown'` waits rather than redirecting: on a reload there is no
 * access token yet (ADR-0001 — it is memory-only) and the app is mid-way
 * through redeeming the refresh cookie. Treating `unknown` as `anonymous` here
 * would bounce every reload of an authenticated session to the login screen,
 * which is the most disruptive regression this dashboard can ship.
 */
router.beforeEach(async (to) => {
  if (to.meta.public) return true

  const auth = useAuthStore()
  if (auth.status === 'unknown') await untilResolved()
  if (auth.isAuthenticated) return true

  return { name: 'login', query: loginQuery(to.fullPath) }
})

/**
 * Where to come back to after signing in.
 *
 * `/` is the default landing route, so carrying it as `next` adds a query
 * parameter that changes nothing — noise in a URL people see at their most
 * suspicious moment. The guard and the reactive watch below must agree about
 * this: they answer the same question and used not to, so the same situation
 * produced two different URLs depending on which half of the rule fired.
 */
function loginQuery(fullPath: string): Record<string, string> {
  return fullPath === '/' ? {} : { next: fullPath }
}

function untilResolved(): Promise<void> {
  const auth = useAuthStore()
  return new Promise((resolve) => {
    const stop = watch(
      () => auth.status,
      (status) => {
        if (status !== 'unknown') {
          stop()
          resolve()
        }
      },
      { immediate: true },
    )
  })
}

/**
 * The half a guard cannot do.
 *
 * `beforeEach` runs on navigation. A session that dies *while the user sits on
 * a page* — an expired refresh token, a revoked session, another tab signing
 * out — produces no navigation, so the guard never fires and the page stays up,
 * silently failing every request behind it.
 *
 * React's `<Navigate>` pattern got this for free by re-rendering on state
 * change. In Vue it has to be said out loud, and `docs/vue-migration-plan.md`
 * lists forgetting it among the top risks. Hence this watch, which is the
 * reactive half of the same rule.
 */
export function installAuthWatch() {
  const auth = useAuthStore()
  watch(
    () => auth.status,
    (status) => {
      if (status !== 'anonymous') return
      if (router.currentRoute.value.meta.public) return
      // A deliberate sign-out navigates itself, with a full reload that drops
      // the query cache and the open streams. Racing it here is how `/login`
      // became `/login?next=/` on a slow enough machine.
      if (auth.signedOut) return
      void router.replace({
        name: 'login',
        query: loginQuery(router.currentRoute.value.fullPath),
      })
    },
  )
}
