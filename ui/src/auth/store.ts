import { create } from 'zustand'

interface AuthState {
  token: string | null
  refreshToken: string | null
  isAuthenticated: boolean
  login: (token: string, refreshToken: string) => void
  logout: () => void
}

export const useAuthStore = create<AuthState>((set) => ({
  token: localStorage.getItem('croniq_token'),
  refreshToken: localStorage.getItem('croniq_refresh'),
  isAuthenticated: !!localStorage.getItem('croniq_token'),
  login: (token, refreshToken) => {
    localStorage.setItem('croniq_token', token)
    localStorage.setItem('croniq_refresh', refreshToken)
    set({ token, refreshToken, isAuthenticated: true })
  },
  logout: () => {
    localStorage.removeItem('croniq_token')
    localStorage.removeItem('croniq_refresh')
    set({ token: null, refreshToken: null, isAuthenticated: false })
  },
}))
