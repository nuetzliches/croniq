import { BrowserRouter, Routes } from 'react-router'
import { MutationCache, QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { appRoutes } from '@/routes'
import { Toaster } from '@/components/ui/toaster'
import { pushApiError } from '@/lib/toast'
import { bootstrap } from '@/auth/session'

// Recover the session before the router renders anything (issue #454). The
// access token is memory-only now, so a reload arrives with none and this is
// what trades the `HttpOnly` refresh cookie for a fresh one. Fired at module
// scope rather than from an effect so it is already in flight while React
// mounts; `ProtectedRoute` renders a spinner until the store leaves 'unknown'.
void bootstrap()

// Surface every failed mutation as a toast. Individual callers can
// still pass their own `onError` to override or augment. A 401 that outlived
// the refresh-and-retry in `apiFetch` / `apiDelete` has already cleared the
// session, so we silence those to avoid a redundant toast on top of the
// redirect to /login.
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
        <Routes>{appRoutes}</Routes>
      </BrowserRouter>
      <Toaster />
    </QueryClientProvider>
  )
}
