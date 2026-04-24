import { useEffect, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useAuthStore } from '@/auth/store'

// See api/client.ts for the rationale — relative URLs by default.
const BASE = import.meta.env.VITE_API_URL ?? ''

interface SSEOptions<T> {
  url: string
  queryKey: unknown[]
  eventType?: string
  onMessage?: (data: T) => void
  enabled?: boolean
}

export function useSSE<T>({ url, queryKey, eventType = 'message', onMessage, enabled = true }: SSEOptions<T>) {
  const qc = useQueryClient()
  const [isConnected, setIsConnected] = useState(false)
  const retryRef = useRef(0)
  const timerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  useEffect(() => {
    if (!enabled) return
    let stopped = false
    let ctrl = new AbortController()

    async function connect() {
      const token = useAuthStore.getState().token
      try {
        const res = await fetch(`${BASE}${url}`, {
          signal: ctrl.signal,
          headers: {
            Accept: 'text/event-stream',
            ...(token ? { Authorization: `Bearer ${token}` } : {}),
          },
        })
        if (res.status === 401) { useAuthStore.getState().logout(); return }
        if (!res.ok || !res.body) throw new Error(`SSE ${res.status}`)

        setIsConnected(true)
        retryRef.current = 0
        const reader = res.body.getReader()
        const dec = new TextDecoder()
        let buf = ''

        while (true) {
          const { done, value } = await reader.read()
          if (done) break
          buf += dec.decode(value, { stream: true })
          const parts = buf.split('\n\n')
          buf = parts.pop() ?? ''
          for (const msg of parts) {
            const lines = msg.split('\n')
            const evtLine = lines.find((l) => l.startsWith('event:'))
            const dataLine = lines.find((l) => l.startsWith('data:'))
            if (!dataLine) continue
            const type = evtLine ? evtLine.slice(6).trim() : 'message'
            if (type !== eventType) continue
            try {
              const parsed: T = JSON.parse(dataLine.slice(5).trim())
              qc.setQueryData(queryKey, parsed)
              onMessage?.(parsed)
            } catch { /* ignore parse errors */ }
          }
        }
      } catch { /* will reconnect */ }
      finally { setIsConnected(false) }

      if (!stopped) {
        const delay = Math.min(1000 * 2 ** retryRef.current, 30_000)
        retryRef.current++
        timerRef.current = setTimeout(() => { ctrl = new AbortController(); connect() }, delay)
      }
    }

    connect()
    return () => { stopped = true; ctrl.abort(); clearTimeout(timerRef.current) }
  }, [url, eventType, enabled, qc, queryKey, onMessage])

  return { isConnected }
}
