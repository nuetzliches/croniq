import { create } from 'zustand'

export interface Toast {
  id: string
  variant: 'error' | 'success' | 'info'
  message: string
}

interface ToastStore {
  toasts: Toast[]
  push: (t: Omit<Toast, 'id'>) => string
  dismiss: (id: string) => void
}

export const useToasts = create<ToastStore>((set) => ({
  toasts: [],
  push: (t) => {
    const id = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
    set((s) => ({ toasts: [...s.toasts, { id, ...t }] }))
    // Auto-dismiss after 6s — long enough to read, short enough not to
    // pile up under a misbehaving job that fails on every retry.
    setTimeout(() => {
      set((s) => ({ toasts: s.toasts.filter((x) => x.id !== id) }))
    }, 6000)
    return id
  },
  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((x) => x.id !== id) })),
}))

/// Convenience wrapper used by App.tsx's mutation cache. Strips the
/// `<status>: <body>` envelope that `apiFetch`/`apiDelete` use so the
/// user sees the server's message instead of a noisy prefix.
export function pushApiError(prefix: string, err: unknown): void {
  const raw = err instanceof Error ? err.message : String(err)
  const m = raw.match(/^\d+:\s*(.+)$/s)
  const body = m ? m[1] : raw
  let message = body
  try {
    const parsed = JSON.parse(body)
    if (parsed && typeof parsed.message === 'string') message = parsed.message
  } catch {
    /* not JSON — keep as-is */
  }
  useToasts.getState().push({ variant: 'error', message: `${prefix}: ${message}` })
}
