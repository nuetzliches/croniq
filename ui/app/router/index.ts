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
      /*
       * Two routes the React tree served and this one does not.
       *
       * `/runners/:runnerId` and `/dead-letters/:id` were real pages until
       * v0.38.0, and the app emitted links to them from entity links and the
       * dashboard — so they are in bookmarks, in alert bodies and in chat
       * history. Without these they land on the not-found page, which reads as
       * "that runner is gone" rather than "that page moved" (issue #669).
       *
       * The runner one drops its id: the screen inventory folded runner detail
       * into the list, so there is nothing to select. Better to arrive at the
       * fleet than at a 404, and the row is on it.
       */
      {
        path: 'runners/:runnerId',
        redirect: () => ({ name: 'runners' }),
      },
      {
        path: 'dead-letters',
        name: 'dead-letters',
        component: () => import('~/pages/DeadLettersView.vue'),
        meta: { title: 'Dead Letters' },
      },
      {
        path: 'dead-letters/:id',
        redirect: (to) => ({ name: 'dead-letters', query: { selected: String(to.params.id) } }),
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
    ],
  },
  {
    /*
     * Where the server's emails land.
     *
     * Both of these links are built server-side — `password_reset.rs` and
     * `invitations.rs` compose `{base}/password-reset/confirm?token=…` and
     * `{base}/invitations/accept?token=…` — and until now **neither tree
     * served the route**. Requesting a reset worked, the mail went out, and
     * the link opened the not-found page; an invited person could not get in
     * at all. The flow was half-built and looked complete from the side an
     * operator tests.
     *
     * Public, necessarily: whoever follows one of these has no session yet.
     */
    path: '/password-reset/confirm',
    name: 'password-reset',
    component: () => import('~/pages/PasswordResetView.vue'),
    meta: { public: true, title: 'Set a new password' },
  },
  {
    path: '/invitations/accept',
    name: 'invitation-accept',
    component: () => import('~/pages/InvitationAcceptView.vue'),
    meta: { public: true, title: 'Accept your invitation' },
  },
  {
    path: '/:pathMatch(.*)*',
    name: 'not-found',
    component: () => import('~/pages/NotFoundView.vue'),
    /*
     * Not `public`.
     *
     * It was, so the guard returned early for it and an unauthenticated
     * visitor opening any unknown URL — a renamed screen, a typo, a bookmark
     * from a version with more routes — got the not-found page instead of the
     * sign-in form. That page sits outside the shell and has no navigation, so
     * their only way forward was the back button (issue #720).
     *
     * Signed in, an unknown URL is still a 404, which is the honest answer.
     */
    meta: { title: 'Not found' },
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
  // An operator who is already signed in has no business on the sign-in form.
  // Landing there is easy — a bookmark, a stale tab, the back button — and the
  // form would happily take a password and post it, which is how a mistyped one
  // used to cost two lockout attempts instead of one (issue #659).
  //
  // The token is not read here: `status` is, so a page reloaded mid-bootstrap
  // waits for the refresh to resolve rather than flashing the form.
  if (to.name === 'login') {
    const auth = useAuthStore()
    if (auth.status === 'unknown') await untilResolved()
    if (auth.isAuthenticated) return { name: 'dashboard' }
    return true
  }

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
      // Nothing has been navigated to yet.
      //
      // Until the first navigation finalises, `currentRoute` is
      // `START_LOCATION`: path `/`, no name, and `meta` an empty object — so
      // the `public` check below cannot see that the target *is* public. A
      // bootstrap refresh answering `no_session` before the lazy chunk of
      // `/invitations/accept?token=…` had loaded would therefore redirect to
      // `/login`, superseding the pending navigation and taking the token with
      // it. The invitee lands on a sign-in form with nothing to sign in with
      // (issue #665).
      //
      // `beforeEach` owns this window and handles it correctly — it has `to`,
      // which `START_LOCATION` is not. `main.ts` also defers installing this
      // watch until `router.isReady()`, so in practice it should never see
      // this state; the check is here because "should never" and "cannot" are
      // different, and the failure mode is losing someone's invitation.
      if (router.currentRoute.value.name === undefined) return
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
