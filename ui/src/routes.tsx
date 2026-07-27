// This module holds the route table, not components — the lazy page handles
// below are chunk boundaries, and its only export is an element tree. That
// trips react-refresh's "only export components" rule; the cost is that edits
// to the route table trigger a full reload instead of HMR, which is what
// changing routes does anyway.
/* eslint-disable react-refresh/only-export-components */
import { lazy } from 'react'
import { Route } from 'react-router'
import { LoginPage } from '@/auth/LoginPage'
import { ProtectedRoute } from '@/auth/ProtectedRoute'
import { Layout } from '@/layout/Layout'
import { NotFoundPage } from '@/pages/NotFoundPage'

// Page-level code splitting. Each route loads its own chunk on demand so
// the initial bundle is just login + layout + the chunks needed for the
// landing route. The Suspense boundary lives in Layout, around <Outlet />,
// so all protected pages share a single fallback spinner.
const DashboardPage = lazy(() =>
  import('@/pages/DashboardPage').then((m) => ({ default: m.DashboardPage })),
)
const JobsPage = lazy(() =>
  import('@/pages/JobsPage').then((m) => ({ default: m.JobsPage })),
)
const RunnersPage = lazy(() =>
  import('@/pages/RunnersPage').then((m) => ({ default: m.RunnersPage })),
)
const DeadLettersPage = lazy(() =>
  import('@/pages/DeadLettersPage').then((m) => ({ default: m.DeadLettersPage })),
)
const ExecutionsPage = lazy(() =>
  import('@/pages/ExecutionsPage').then((m) => ({ default: m.ExecutionsPage })),
)
const AlertsPage = lazy(() =>
  import('@/pages/AlertsPage').then((m) => ({ default: m.AlertsPage })),
)
const CalendarsPage = lazy(() =>
  import('@/pages/CalendarsPage').then((m) => ({ default: m.CalendarsPage })),
)
const SettingsPage = lazy(() =>
  import('@/pages/SettingsPage').then((m) => ({ default: m.SettingsPage })),
)
const ConsolePage = lazy(() =>
  import('@/pages/ConsolePage').then((m) => ({ default: m.ConsolePage })),
)
// NotFoundPage is the one page imported eagerly: it is a few lines of markup,
// and being the last-resort fallback it should never depend on a chunk fetch
// that could itself fail.

/**
 * The route table, kept out of App.tsx so `routes.test.ts` can assert what a
 * path resolves to without standing up the providers.
 */
export const appRoutes = (
  <>
    <Route path="/login" element={<LoginPage />} />
    <Route element={<ProtectedRoute />}>
      <Route element={<Layout />}>
        <Route path="/" element={<DashboardPage />} />
        <Route path="/jobs" element={<JobsPage />} />
        <Route path="/jobs/:jobKey" element={<JobsPage />} />
        <Route path="/runners" element={<RunnersPage />} />
        <Route path="/runners/:runnerId" element={<RunnersPage />} />
        <Route path="/dead-letters" element={<DeadLettersPage />} />
        <Route path="/dead-letters/:id" element={<DeadLettersPage />} />
        <Route path="/executions" element={<ExecutionsPage />} />
        <Route path="/executions/:id" element={<ExecutionsPage />} />
        <Route path="/alerts" element={<AlertsPage />} />
        <Route path="/calendars" element={<CalendarsPage />} />
        <Route path="/console" element={<ConsolePage />} />
        <Route path="/settings" element={<SettingsPage />} />
        {/* Unknown path. Nested this deep on purpose: an authenticated
            operator keeps the sidebar and a way back, and a logged-out one
            is bounced to /login by ProtectedRoute like any other page. */}
        <Route path="*" element={<NotFoundPage />} />
      </Route>
    </Route>
  </>
)
