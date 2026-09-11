import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router'
import { watch } from 'vue'
import { useAuthStore } from '~/stores/auth'

/**
 * Routes follow the screen set agreed in `docs/ui-screen-inventory.md`.
 *
 * Screens not yet built render `NotBuiltYet` rather than 404ing. During the
 * rebuild the shell has to be walkable to be judged at all, and a page that
 * says which step it belongs to is honest in a way an empty route is not —
 * nobody mistakes it for finished.
 */
const placeholder = () => import('~/pages/NotBuiltYet.vue')

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
      { path: '', name: 'dashboard', component: placeholder, meta: { title: 'Dashboard', step: 4 } },
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
      { path: 'runners', name: 'runners', component: placeholder, meta: { title: 'Runners', step: 4 } },
      { path: 'dead-letters', name: 'dead-letters', component: placeholder, meta: { title: 'Dead Letters', step: 4 } },
      { path: 'jobs', name: 'jobs', component: placeholder, meta: { title: 'Jobs', step: 5 } },
      { path: 'jobs/:jobKey', name: 'job', component: placeholder, meta: { title: 'Job', step: 5 } },
      { path: 'calendars', name: 'calendars', component: placeholder, meta: { title: 'Calendars', step: 6 } },
      { path: 'alerts', name: 'alerts', component: placeholder, meta: { title: 'Alerts', step: 6 } },
      { path: 'console', name: 'console', component: placeholder, meta: { title: 'Console', step: 6 } },
      { path: 'settings', name: 'settings', component: placeholder, meta: { title: 'Settings', step: 6 } },
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

  return { name: 'login', query: to.fullPath === '/' ? {} : { next: to.fullPath } }
})

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
      void router.replace({
        name: 'login',
        query: { next: router.currentRoute.value.fullPath },
      })
    },
  )
}
