import { useAuthStore } from '@/auth/store'
import { useNavigate } from 'react-router'

export function Header() {
  const logout = useAuthStore((s) => s.logout)
  const navigate = useNavigate()

  function handleLogout() {
    logout()
    navigate('/login')
  }

  return (
    <header className="h-12 border-b border-border px-4 flex items-center justify-end bg-card">
      <button
        onClick={handleLogout}
        className="text-sm text-muted-foreground hover:text-foreground"
      >
        Logout
      </button>
    </header>
  )
}
