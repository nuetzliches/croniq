import { BrowserRouter, Routes } from 'react-router'
import { MutationCache, QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { appRoutes } from '@/routes'
import { Toaster } from '@/components/ui/toaster'
import { pushApiError } from '@/lib/toast'

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
        <Routes>{appRoutes}</Routes>
      </BrowserRouter>
      <Toaster />
    </QueryClientProvider>
  )
}
