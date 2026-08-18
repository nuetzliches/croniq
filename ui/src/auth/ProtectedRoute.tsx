import { Navigate, Outlet } from 'react-router'
import { Spinner } from '@/components/ui/spinner'
import { useAuthStore } from './store'

/**
 * Gate for the authenticated routes.
 *
 * The `'unknown'` branch is what makes #454 usable: with the access token in
 * memory only, every reload starts signed-out and the session is recovered by
 * `session.bootstrap()` a moment later. Redirecting on a falsy token would
 * bounce every reload to /login before that answer arrives.
 */
export function ProtectedRoute() {
  const status = useAuthStore((s) => s.status)
  if (status === 'unknown') {
    return (
      <div style={{ display: 'grid', placeItems: 'center', minHeight: '100vh' }}>
        <Spinner className="h-6 w-6 text-muted-foreground" />
      </div>
    )
  }
  if (status === 'anonymous') return <Navigate to="/login" replace />
  return <Outlet />
}
