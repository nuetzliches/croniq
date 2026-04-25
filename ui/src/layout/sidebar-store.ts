import { create } from 'zustand'
import { persist } from 'zustand/middleware'

interface SidebarStore {
  /// Whether the user has explicitly collapsed the desktop sidebar.
  /// Below the `lg` breakpoint the sidebar is forced to icon-only mode
  /// regardless of this flag — see `Sidebar.tsx` for the merge.
  collapsed: boolean
  toggle: () => void

  /// Mobile-only: drawer is closed by default and toggled via the header
  /// hamburger. Not persisted — should always start closed on a fresh load.
  mobileOpen: boolean
  setMobileOpen: (open: boolean) => void
  toggleMobile: () => void
}

export const useSidebarStore = create<SidebarStore>()(
  persist(
    (set) => ({
      collapsed: false,
      toggle: () => set((s) => ({ collapsed: !s.collapsed })),
      mobileOpen: false,
      setMobileOpen: (mobileOpen) => set({ mobileOpen }),
      toggleMobile: () => set((s) => ({ mobileOpen: !s.mobileOpen })),
    }),
    {
      name: 'croniq_sidebar',
      // Only persist the desktop preference. The mobile drawer always
      // starts closed on a fresh load.
      partialize: (s) => ({ collapsed: s.collapsed }),
    }
  )
)
