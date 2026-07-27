import { Link, useLocation } from 'react-router'
import { Compass } from 'lucide-react'
import { EmptyState } from '@/components/ui/empty-state'
import { truncate } from '@/lib/utils'

/**
 * Catch-all for paths that match no route.
 *
 * It is mounted inside <Layout>, so the sidebar and topbar stay put: a typo'd
 * URL or a stale bookmark reads as "wrong address" with a way back, instead of
 * the blank page it used to render. The server already falls back to
 * index.html for unknown paths, so this is the only 404 an operator sees.
 */
export function NotFoundPage() {
  const { pathname } = useLocation()
  return (
    <div className="page">
      <EmptyState
        icon={<Compass className="h-10 w-10" />}
        title="Page not found"
        description={`Nothing is served at ${truncate(pathname, 120)} — check the address, or head back to the dashboard.`}
        action={
          <Link to="/" className="btn primary">
            Back to dashboard
          </Link>
        }
      />
    </div>
  )
}
