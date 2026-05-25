import { Suspense, useEffect, useState } from 'react'
import { Outlet } from 'react-router'
import { Sidebar } from './Sidebar'
import { Topbar } from './Topbar'
import { Spinner } from '@/components/ui/spinner'
import { useSidebarStore } from './sidebar-store'
import { CommandPalette } from './CommandPalette'

export function Layout() {
  const collapsed = useSidebarStore((s) => s.collapsed)
  const [paletteOpen, setPaletteOpen] = useState(false)

  // Global ⌘K / Ctrl+K opens the palette. The Topbar's search trigger also
  // opens its own local instance — that's fine; the modal is mounted from
  // wherever it's triggered.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault()
        setPaletteOpen(true)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  return (
    <div className="app" data-sidebar={collapsed ? 'collapsed' : undefined}>
      <div className="app-bg" aria-hidden="true">
        <div className="app-spot app-spot-a" />
        <div className="app-spot app-spot-b" />
      </div>
      <Sidebar />
      <Topbar />
      <main className="main">
        <Suspense fallback={<PageFallback />}>
          <Outlet />
        </Suspense>
      </main>
      {paletteOpen ? <CommandPalette onClose={() => setPaletteOpen(false)} /> : null}
    </div>
  )
}

function PageFallback() {
  return (
    <div style={{ display: 'grid', placeItems: 'center', minHeight: '40vh' }}>
      <Spinner className="h-6 w-6 text-muted-foreground" />
    </div>
  )
}
