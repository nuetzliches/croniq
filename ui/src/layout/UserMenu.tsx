import { useEffect } from 'react'
import { useNavigate } from 'react-router'
import { Sun, Moon, User as UserIcon, Key, LogOut, Globe } from 'lucide-react'
import { useAuthStore } from '@/auth/store'
import { useTheme } from '@/lib/theme'
import clsx from 'clsx'


export interface UserMenuProps {
  onClose: () => void
}

export function UserMenu({ onClose }: UserMenuProps) {
  const navigate = useNavigate()
  const logout = useAuthStore((s) => s.logout)
  const { pref, setPref } = useTheme()

  useEffect(() => {
    // Delay the click-away listener by one tick so the same click that
    // opened the menu doesn't immediately close it.
    let armed = false
    const arm = window.setTimeout(() => {
      armed = true
    }, 100)
    function onAway(e: MouseEvent) {
      if (!armed) return
      const t = e.target as HTMLElement
      if (t.closest('.user-menu') || t.closest('.user-pill')) return
      onClose()
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('mousedown', onAway)
    document.addEventListener('keydown', onKey)
    return () => {
      window.clearTimeout(arm)
      document.removeEventListener('mousedown', onAway)
      document.removeEventListener('keydown', onKey)
    }
  }, [onClose])

  function go(path: string) {
    onClose()
    navigate(path)
  }
  function signOut() {
    onClose()
    logout()
    navigate('/login')
  }

  return (
    <div className="user-menu" role="menu">
      <button type="button" className="user-menu-item" onClick={() => go('/settings?tab=profile')}>
        <UserIcon size={14} />
        <span>Profile &amp; account</span>
      </button>
      <button type="button" className="user-menu-item" onClick={() => go('/settings?tab=clients')}>
        <Key size={14} />
        <span>API keys &amp; clients</span>
      </button>

      <div className="user-menu-sep" />

      <div className="user-menu-theme">
        <span className="dim" style={{ fontSize: 11.5, padding: '0 6px' }}>
          Appearance
        </span>
        <div className="user-menu-theme-seg" role="radiogroup" aria-label="Theme">
          <button
            type="button"
            role="radio"
            aria-checked={pref === 'light'}
            className={clsx(pref === 'light' && 'active')}
            onClick={() => setPref('light')}
          >
            <Sun size={12} /> Light
          </button>
          <button
            type="button"
            role="radio"
            aria-checked={pref === 'dark'}
            className={clsx(pref === 'dark' && 'active')}
            onClick={() => setPref('dark')}
          >
            <Moon size={12} /> Dark
          </button>
        </div>
      </div>

      <button
        type="button"
        className="user-menu-item"
        onClick={() => {
          window.open('https://nuetzliches.github.io/croniq/', '_blank', 'noopener,noreferrer')
        }}
      >
        <Globe size={14} />
        <span>Documentation</span>
      </button>

      <div className="user-menu-sep" />

      <button type="button" className="user-menu-item danger" onClick={signOut}>
        <LogOut size={14} />
        <span>Sign out</span>
      </button>
    </div>
  )
}

