import { BrowserRouter, Routes, Route } from 'react-router'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { LoginPage } from '@/auth/LoginPage'
import { ProtectedRoute } from '@/auth/ProtectedRoute'
import { Layout } from '@/layout/Layout'
import { DashboardPage } from '@/pages/DashboardPage'
import { JobsPage } from '@/pages/JobsPage'
import { JobDetailPage } from '@/pages/JobDetailPage'
import { RunnersPage } from '@/pages/RunnersPage'
import { DeadLettersPage } from '@/pages/DeadLettersPage'
import { ExecutionsPage } from '@/pages/ExecutionsPage'
import { CalendarsPage } from '@/pages/CalendarsPage'
import { SettingsPage } from '@/pages/SettingsPage'

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: 1, refetchOnWindowFocus: false },
  },
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
              <Route path="/jobs/:jobKey" element={<JobDetailPage />} />
              <Route path="/runners" element={<RunnersPage />} />
              <Route path="/dead-letters" element={<DeadLettersPage />} />
              <Route path="/executions" element={<ExecutionsPage />} />
              <Route path="/calendars" element={<CalendarsPage />} />
              <Route path="/settings" element={<SettingsPage />} />
            </Route>
          </Route>
        </Routes>
      </BrowserRouter>
    </QueryClientProvider>
  )
}
