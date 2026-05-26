import { lazy } from 'react'
import { BrowserRouter, Routes, Route } from 'react-router'
import { MutationCache, QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { LoginPage } from '@/auth/LoginPage'
import { ProtectedRoute } from '@/auth/ProtectedRoute'
import { Layout } from '@/layout/Layout'
import { Toaster } from '@/components/ui/toaster'
import { pushApiError } from '@/lib/toast'

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

// Surface every failed mutation as a toast. Individual callers can
// still pass their own `onError` to override or augment. Auth-401
// already triggers `useAuthStore.logout()` inside `apiFetch` /
// `apiDelete`, so we silence those to avoid a redundant toast on top
// of the redirect to /login.
const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: 1, refetchOnWindowFocus: false },
  },
  mutationCache: new MutationCache({
    onError: (err, _vars, _ctx, mutation) => {
      const msg = err instanceof Error ? err.message : String(err)
      if (msg === 'Unauthorized') return
      const verb = mutation.options.meta?.action ?? 'Request failed'
      pushApiError(String(verb), err)
    },
  }),
})

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <Routes>
          <Route path="/login" element={<LoginPage />} />
          <Route element={<ProtectedRoute />}>
            <Route element={<Layout />}>
              <Route path="/" element={<DashboardPage />} />
              <Route path="/jobs" element={<JobsPage />} />
              <Route path="/jobs/:jobKey" element={<JobsPage />} />
              <Route path="/runners" element={<RunnersPage />} />
              <Route path="/dead-letters" element={<DeadLettersPage />} />
              <Route path="/dead-letters/:id" element={<DeadLettersPage />} />
              <Route path="/executions" element={<ExecutionsPage />} />
              <Route path="/alerts" element={<AlertsPage />} />
              <Route path="/calendars" element={<CalendarsPage />} />
              <Route path="/settings" element={<SettingsPage />} />
            </Route>
          </Route>
        </Routes>
      </BrowserRouter>
      <Toaster />
    </QueryClientProvider>
  )
}
