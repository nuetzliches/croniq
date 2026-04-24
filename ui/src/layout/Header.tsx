import { useLocation, useNavigate } from 'react-router'
import { Sun, Moon, Bell, LogOut } from 'lucide-react'
import * as DropdownMenu from '@radix-ui/react-dropdown-menu'
import * as Tooltip from '@radix-ui/react-tooltip'
import { useAuthStore } from '@/auth/store'
import { useTheme } from '@/lib/theme'
import { useDeadLetters } from '@/api/hooks'
import { cn } from '@/lib/utils'

const routeTitles: Record<string, string> = {
  '/': 'Dashboard',
  '/jobs': 'Jobs',
  '/runners': 'Runners',
  '/executions': 'Executions',
  '/dead-letters': 'Dead Letters',
  '/calendars': 'Calendars',
  '/settings': 'Settings',
}

function usePageTitle() {
  const { pathname } = useLocation()
  if (pathname.startsWith('/jobs/')) return 'Job Detail'
  return routeTitles[pathname] ?? ''
}

function IconButton({ label, onClick, children }: { label: string; onClick?: () => void; children: React.ReactNode }) {
  return (
    <Tooltip.Provider delayDuration={200}>
      <Tooltip.Root>
        <Tooltip.Trigger asChild>
          <button
            aria-label={label}
            onClick={onClick}
            className="relative flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-accent-foreground transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          >
            {children}
          </button>
        </Tooltip.Trigger>
        <Tooltip.Portal>
          <Tooltip.Content
            side="bottom"
            className="z-50 rounded-md bg-foreground px-2 py-1 text-xs text-background shadow-md"
          >
            {label}
            <Tooltip.Arrow className="fill-foreground" />
          </Tooltip.Content>
        </Tooltip.Portal>
      </Tooltip.Root>
    </Tooltip.Provider>
  )
}

export function Header() {
  const logout = useAuthStore((s) => s.logout)
  const navigate = useNavigate()
  const { theme, toggle } = useTheme()
  const title = usePageTitle()
  const { data: deadLetters } = useDeadLetters()
  const dlCount = deadLetters?.length ?? 0

  function handleLogout() {
    logout()
    navigate('/login')
  }

  return (
    <header className="h-12 border-b border-border px-4 flex items-center justify-between bg-card shrink-0">
      <span className="text-sm font-medium">{title}</span>

      <div className="flex items-center gap-1">
        {/* Dead letter bell */}
        <DropdownMenu.Root>
          <DropdownMenu.Trigger asChild>
            <button
              aria-label={`Dead letters${dlCount > 0 ? `: ${dlCount} pending` : ''}`}
              className="relative flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-accent-foreground transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
            >
              <Bell className="h-4 w-4" />
              {dlCount > 0 && (
                <span
                  aria-live="assertive"
                  className="absolute -top-0.5 -right-0.5 flex h-4 w-4 items-center justify-center rounded-full bg-destructive text-white text-[10px] font-bold"
                >
                  {dlCount > 9 ? '9+' : dlCount}
                </span>
              )}
            </button>
          </DropdownMenu.Trigger>
          <DropdownMenu.Portal>
            <DropdownMenu.Content
              align="end"
              className="z-50 w-72 rounded-lg border border-border bg-card shadow-lg p-1"
            >
              <div className="px-3 py-2 text-xs font-semibold text-muted-foreground uppercase tracking-wide">
                Dead Letters
              </div>
              {dlCount === 0 ? (
                <div className="px-3 py-4 text-center text-xs text-muted-foreground">No dead letters</div>
              ) : (
                <>
                  {(deadLetters ?? []).slice(0, 3).map((dl) => (
                    <DropdownMenu.Item
                      key={dl.id}
                      className={cn(
                        'flex flex-col gap-0.5 rounded-md px-3 py-2 text-xs cursor-pointer',
                        'hover:bg-accent focus:bg-accent outline-none'
                      )}
                      onSelect={() => navigate('/dead-letters')}
                    >
                      <span className="font-medium text-foreground">{dl.job_key}</span>
                      <span className="text-muted-foreground truncate">{dl.error}</span>
                    </DropdownMenu.Item>
                  ))}
                  {dlCount > 3 && (
                    <DropdownMenu.Item
                      className="rounded-md px-3 py-2 text-xs text-primary cursor-pointer hover:bg-accent focus:bg-accent outline-none"
                      onSelect={() => navigate('/dead-letters')}
                    >
                      View all {dlCount} dead letters →
                    </DropdownMenu.Item>
                  )}
                </>
              )}
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>

        {/* Theme toggle */}
        <IconButton label={theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'} onClick={toggle}>
          {theme === 'dark' ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
        </IconButton>

        {/* Logout */}
        <IconButton label="Log out" onClick={handleLogout}>
          <LogOut className="h-4 w-4" />
        </IconButton>
      </div>
    </header>
  )
}
