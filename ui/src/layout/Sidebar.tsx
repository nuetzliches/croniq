import { NavLink } from 'react-router'

const links = [
  { to: '/', label: 'Dashboard', icon: '~' },
  { to: '/jobs', label: 'Jobs', icon: 'J' },
  { to: '/schedules', label: 'Schedules', icon: 'S' },
  { to: '/runners', label: 'Runners', icon: 'R' },
  { to: '/executions', label: 'Executions', icon: 'E' },
  { to: '/dead-letters', label: 'Dead Letters', icon: 'D' },
]

export function Sidebar() {
  return (
    <aside className="w-56 border-r border-border bg-card flex flex-col">
      <div className="p-4 border-b border-border flex items-center gap-2">
        <img src="/favicon.svg" alt="Croniq" className="h-6 w-6" />
        <span className="font-semibold text-sm">Croniq</span>
      </div>
      <nav className="flex-1 p-2 space-y-0.5">
        {links.map((link) => (
          <NavLink
            key={link.to}
            to={link.to}
            end={link.to === '/'}
            className={({ isActive }) =>
              `flex items-center gap-2 px-3 py-2 rounded-md text-sm ${
                isActive
                  ? 'bg-accent text-accent-foreground font-medium'
                  : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground'
              }`
            }
          >
            <span className="w-5 text-center font-mono text-xs">{link.icon}</span>
            {link.label}
          </NavLink>
        ))}
      </nav>
    </aside>
  )
}
