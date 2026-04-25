import { NavLink, useLocation, useMatch } from 'react-router'
import { useEffect } from 'react'
import * as Tooltip from '@radix-ui/react-tooltip'
import {
  LayoutDashboard, Briefcase, CalendarDays, Cpu, List, MailX,
  Settings, PanelLeftClose, PanelLeftOpen, X,
} from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useMediaQuery } from '@/lib/use-media-query'
import { useDeadLetters } from '@/api/hooks'
import { useSidebarStore } from './sidebar-store'

interface NavLinkSpec {
  to: string
  label: string
  icon: LucideIcon
  // Render a count badge to the right of the label. Drawn as a small dot
  // when collapsed so the icon-only rail still hints at pending work.
  badge?: number
}

const baseLinks: Omit<NavLinkSpec, 'badge'>[] = [
  { to: '/', label: 'Dashboard', icon: LayoutDashboard },
  { to: '/jobs', label: 'Jobs', icon: Briefcase },
  { to: '/runners', label: 'Runners', icon: Cpu },
  { to: '/executions', label: 'Executions', icon: List },
  { to: '/dead-letters', label: 'Dead Letters', icon: MailX },
  { to: '/calendars', label: 'Calendars', icon: CalendarDays },
  { to: '/settings', label: 'Settings', icon: Settings },
]

// The nav item pre-computes `isActive` and passes a plain string className.
// NavLink's function-className form gets stringified by `<Tooltip.Trigger
// asChild>` (Radix Slot merges props by joining classNames as strings),
// which collapses the callback's source into the DOM class attribute —
// causing every item to render with a bare `text-primary` token and broken
// padding tokens like `p-2.5:px-3`.
function NavItem({ link, collapsed }: { link: NavLinkSpec; collapsed: boolean }) {
  const end = link.to === '/'
  const match = useMatch({ path: link.to, end })
  const isActive = match !== null
  const Icon = link.icon

  // Same padding in both states keeps the icon at a fixed x-offset.
  // If we switched to `justify-center p-2.5` while the sidebar width is
  // still mid-transition, every icon would snap to the middle of the
  // still-wide sidebar and slide back as it narrows — visible as a jump.
  // `h-9` locks the item height across states: expanded items naturally
  // compute to 36px (text line-height 20 + 16 padding), collapsed items
  // would otherwise shrink to 32px because the hidden span removes the
  // line-height contribution, making the nav total height flicker on
  // collapse.
  // `whitespace-nowrap overflow-hidden` keeps two-word labels (Dead
  // Letters) from wrapping to a second line during early expand frames
  // when the item is still narrow. Without it the span breaks at the
  // space, jumps to 40px tall, and `items-center` re-seats the icon —
  // visible as a Dead-Letters-only layout twitch.
  const className = cn(
    'flex items-center gap-2.5 rounded-md text-sm transition-colors mx-2 px-3 h-9',
    'overflow-hidden whitespace-nowrap',
    isActive
      ? 'bg-primary/10 text-primary font-medium'
      : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground'
  )

  const navLink = (
    <NavLink
      to={link.to}
      end={end}
      className={cn(className, 'relative')}
      aria-label={collapsed ? `${link.label}${link.badge ? `, ${link.badge} pending` : ''}` : undefined}
    >
      <Icon className="h-4 w-4 shrink-0" />
      {!collapsed && <span className="flex-1">{link.label}</span>}
      {!collapsed && link.badge ? (
        <span
          aria-label={`${link.badge} pending`}
          className="ml-auto inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-destructive px-1 text-[10px] font-bold text-white"
        >
          {link.badge > 9 ? '9+' : link.badge}
        </span>
      ) : null}
      {collapsed && link.badge ? (
        // Tiny dot in the icon-only rail — same `bg-destructive` as the
        // expanded badge so the visual cue carries across breakpoints.
        <span
          aria-hidden="true"
          className="absolute right-1.5 top-1.5 h-1.5 w-1.5 rounded-full bg-destructive"
        />
      ) : null}
    </NavLink>
  )

  if (!collapsed) return navLink

  return (
    <Tooltip.Root>
      <Tooltip.Trigger asChild>{navLink}</Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content
          side="right"
          className="z-50 rounded-md bg-foreground px-2.5 py-1 text-xs text-background shadow-md"
        >
          {link.label}
          <Tooltip.Arrow className="fill-foreground" />
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  )
}

export function Sidebar() {
  const collapsedPref = useSidebarStore((s) => s.collapsed)
  const toggle = useSidebarStore((s) => s.toggle)
  const mobileOpen = useSidebarStore((s) => s.mobileOpen)
  const setMobileOpen = useSidebarStore((s) => s.setMobileOpen)
  const { data: deadLetters } = useDeadLetters()
  const dlCount = deadLetters?.length ?? 0
  // Splice the dead-letter count onto the matching nav entry. Computed
  // each render — cheap, and avoids stashing the count in a separate
  // store just for the badge.
  const links: NavLinkSpec[] = baseLinks.map((l) =>
    l.to === '/dead-letters' && dlCount > 0 ? { ...l, badge: dlCount } : l
  )

  // Tailwind's `lg` breakpoint is 1024px. Below that the sidebar is
  // forced to icon-only — even on tablet there's not enough room to
  // give 224px to navigation. Below `md` (768px) we go further and
  // hide the rail entirely, surfacing it via the header hamburger as
  // an overlay drawer.
  const isDesktop = useMediaQuery('(min-width: 1024px)')
  const isMobile = useMediaQuery('(max-width: 767px)')

  const collapsed = !isDesktop || collapsedPref

  // Auto-close the mobile drawer on navigation so the next page is
  // immediately readable (otherwise the overlay covers half the
  // viewport).
  const { pathname } = useLocation()
  useEffect(() => {
    if (mobileOpen) setMobileOpen(false)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pathname])

  // Lock body scroll while the mobile drawer is open so background
  // content doesn't bleed-scroll under it.
  useEffect(() => {
    if (!isMobile) return
    document.body.style.overflow = mobileOpen ? 'hidden' : ''
    return () => {
      document.body.style.overflow = ''
    }
  }, [isMobile, mobileOpen])

  // Keep the transform on inline style instead of Tailwind utilities.
  // Tailwind 4 emits `translate-x-0` and `-translate-x-full` into the
  // same `@layer utilities` bucket; depending on JIT generation order,
  // `-translate-x-full` can end up *after* `translate-x-0` in the
  // stylesheet and win even when only `translate-x-0` is in the
  // className. Inline `style` sidesteps the cascade entirely.
  const aside = (
    <aside
      className={cn(
        'border-r border-border bg-card flex flex-col shrink-0',
        collapsed ? 'w-14' : 'w-56',
        isMobile ? 'fixed inset-y-0 left-0 z-50' : 'static'
      )}
      // Inline style wins over Tailwind 4's `translate-x-*` utilities,
      // which proved fragile here (their generated CSS sometimes wins
      // over inline class swaps depending on stylesheet order). Snap
      // into place — animating a fixed-position drawer is nice but not
      // worth the cascade fight.
      style={
        isMobile
          ? { translate: mobileOpen ? '0' : '-100%' }
          : undefined
      }
      aria-label="Main navigation"
    >
      <div className="border-b border-border flex items-center gap-2 h-12 pl-4 pr-2">
        <img src="/favicon.svg" alt="Croniq" className="h-6 w-6 shrink-0" />
        {!collapsed && <span className="font-semibold text-sm flex-1">Croniq</span>}
        {isMobile && mobileOpen && (
          <button
            onClick={() => setMobileOpen(false)}
            aria-label="Close navigation"
            className="ml-auto p-1.5 rounded-md text-muted-foreground hover:bg-accent hover:text-accent-foreground"
          >
            <X className="h-4 w-4" />
          </button>
        )}
      </div>

      <nav className="flex-1 py-2 space-y-0.5" aria-label="Main navigation">
        {links.map((link) => (
          <NavItem key={link.to} link={link} collapsed={collapsed} />
        ))}
      </nav>

      {/* Toggle is desktop-only — on smaller viewports the collapsed
          state is forced and the user controls the drawer instead. */}
      {isDesktop && (
        <div className="flex justify-end px-2 pb-2">
          <Tooltip.Root>
            <Tooltip.Trigger asChild>
              <button
                onClick={toggle}
                aria-label={collapsedPref ? 'Expand sidebar' : 'Collapse sidebar'}
                aria-expanded={!collapsedPref}
                className="p-2 rounded-md text-muted-foreground hover:bg-accent hover:text-accent-foreground transition-colors"
              >
                {collapsedPref
                  ? <PanelLeftOpen className="h-4 w-4" />
                  : <PanelLeftClose className="h-4 w-4" />
                }
              </button>
            </Tooltip.Trigger>
            <Tooltip.Portal>
              <Tooltip.Content
                side="right"
                className="z-50 rounded-md bg-foreground px-2.5 py-1 text-xs text-background shadow-md"
              >
                {collapsedPref ? 'Expand' : 'Collapse'}
                <Tooltip.Arrow className="fill-foreground" />
              </Tooltip.Content>
            </Tooltip.Portal>
          </Tooltip.Root>
        </div>
      )}
    </aside>
  )

  return (
    <Tooltip.Provider delayDuration={0}>
      {aside}
      {/* Backdrop for the mobile drawer — clicking it closes the rail.
          Rendered as a sibling so it doesn't interfere with the slide
          transform on `aside`. */}
      {isMobile && mobileOpen && (
        <button
          onClick={() => setMobileOpen(false)}
          aria-label="Close navigation backdrop"
          className="fixed inset-0 z-40 bg-black/40"
        />
      )}
    </Tooltip.Provider>
  )
}
