import { Suspense } from 'react'
import { Outlet } from 'react-router'
import { Sidebar } from './Sidebar'
import { Header } from './Header'
import { Spinner } from '@/components/ui/spinner'

export function Layout() {
  return (
    <div className="flex h-screen">
      <Sidebar />
      <div className="flex-1 flex flex-col overflow-hidden">
        <Header />
        <main className="flex-1 overflow-auto p-6">
          <Suspense fallback={<PageFallback />}>
            <Outlet />
          </Suspense>
        </main>
      </div>
    </div>
  )
}

function PageFallback() {
  return (
    <div className="flex items-center justify-center min-h-[40vh]">
      <Spinner className="h-6 w-6 text-muted-foreground" />
    </div>
  )
}
