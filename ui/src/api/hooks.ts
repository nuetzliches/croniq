import { useState, useEffect, useRef } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { apiFetch, apiPost, apiPut, apiDelete } from './client'
import type * as T from './types'
import { useAuthStore } from '@/auth/store'

// Health
export function useHealth() {
  return useQuery({ queryKey: ['health'], queryFn: () => apiFetch<T.HealthResponse>('/health'), refetchInterval: 5000 })
}

// Jobs
export function useJobs() {
  return useQuery({ queryKey: ['jobs'], queryFn: () => apiFetch<T.JobDefinition[]>('/v1/jobs') })
}
export function useJob(jobKey: string) {
  return useQuery({ queryKey: ['jobs', jobKey], queryFn: () => apiFetch<T.JobDefinition>(`/v1/jobs/${jobKey}`) })
}
export function useCreateJob() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: { job_key: string; description?: string }) => apiPost('/v1/jobs', data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['jobs'] }),
    meta: { action: 'Create job' },
  })
}
export function useDeleteJob() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (jobKey: string) => apiDelete(`/v1/jobs/${jobKey}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['jobs'] }),
    meta: { action: 'Delete job' },
  })
}
export function useUpdateJob() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({
      job_key,
      ...patch
    }: {
      job_key: string
      description?: string | null
      timeout?: string | null
      max_retries?: number | null
      dead_letter_enabled?: boolean | null
    }) => apiPut<T.JobDefinition>(`/v1/jobs/${job_key}`, patch),
    onSuccess: (_data, vars) => {
      qc.invalidateQueries({ queryKey: ['jobs'] })
      qc.invalidateQueries({ queryKey: ['jobs', vars.job_key] })
    },
    meta: { action: 'Update job' },
  })
}
export function useActivateJob() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (jobKey: string) => apiPost<T.JobDefinition>(`/v1/jobs/${jobKey}/activate`, {}),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['jobs'] }),
    meta: { action: 'Activate job' },
  })
}
export function useDeactivateJob() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (jobKey: string) => apiPost<T.JobDefinition>(`/v1/jobs/${jobKey}/deactivate`, {}),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['jobs'] }),
    meta: { action: 'Deactivate job' },
  })
}
export function useRegisterJob() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: {
      job_key: string
      schedule: string
      timezone?: string
      timeout?: string
      description?: string
      calendar?: string
    }) => apiPost('/v1/jobs/register', data),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['jobs'] }); qc.invalidateQueries({ queryKey: ['schedules'] }) },
    meta: { action: 'Register job' },
  })
}
export function useTriggerJob() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (jobKey: string) => apiPost<T.TriggerResponse>('/v1/trigger', { job_key: jobKey }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['executions'] }),
    meta: { action: 'Trigger job' },
  })
}

// Schedules
export function useSchedules(jobKey?: string) {
  const params = jobKey ? `?job_key=${jobKey}` : ''
  return useQuery({ queryKey: ['schedules', jobKey], queryFn: () => apiFetch<T.TriggerDefinition[]>(`/v1/schedules${params}`) })
}
export function useCreateSchedule() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: {
      job_key: string
      cron_expression: string
      timezone?: string
      calendar?: string
    }) => apiPost('/v1/schedules', data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['schedules'] }),
    meta: { action: 'Create schedule' },
  })
}
export function useUpdateSchedule() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({
      trigger_id,
      ...patch
    }: {
      trigger_id: string
      cron_expression?: string
      timezone?: string
      calendar?: string | null
      enabled?: boolean
    }) => apiPut<T.TriggerDefinition>(`/v1/schedules/${trigger_id}`, patch),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['schedules'] }),
    meta: { action: 'Update schedule' },
  })
}
export function useDeleteSchedule() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => apiDelete(`/v1/schedules/${id}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['schedules'] }),
    meta: { action: 'Delete schedule' },
  })
}

// Runners
export function useRunners() {
  return useQuery({ queryKey: ['runners'], queryFn: () => apiFetch<T.RunnerSummary[]>('/v1/runners'), refetchInterval: 10000 })
}
export function useDeleteRunner() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => apiDelete(`/v1/runners/${id}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['runners'] }),
    meta: { action: 'Delete runner' },
  })
}
export function useRunnersSSE() {
  const qc = useQueryClient()
  const [data, setData] = useState<T.RunnerSummary[] | undefined>()
  const [isConnected, setIsConnected] = useState(false)
  const retryRef = useRef(0)
  const timerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  useEffect(() => {
    let stopped = false
    let ctrl = new AbortController()
    const BASE = import.meta.env.VITE_API_URL ?? ''

    async function connect() {
      const token = useAuthStore.getState().token
      try {
        const res = await fetch(`${BASE}/v1/runners/stream`, {
          signal: ctrl.signal,
          headers: { Accept: 'text/event-stream', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
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
            const line = msg.split('\n').find(l => l.startsWith('data:'))
            if (!line) continue
            try {
              const runners: T.RunnerSummary[] = JSON.parse(line.slice(5).trim())
              setData(runners)
              qc.setQueryData(['runners'], runners)
            } catch { /* ignore parse errors */ }
          }
        }
      } catch { /* will reconnect below */ }
      finally { setIsConnected(false) }

      if (!stopped) {
        const delay = Math.min(1000 * 2 ** retryRef.current, 30_000)
        retryRef.current++
        timerRef.current = setTimeout(() => { ctrl = new AbortController(); connect() }, delay)
      }
    }

    connect()
    return () => { stopped = true; ctrl.abort(); clearTimeout(timerRef.current) }
  }, [qc])

  return { data, isConnected }
}

// Executions
export function useExecutions(params?: { job_key?: string; state?: string; limit?: number }) {
  const search = new URLSearchParams()
  if (params?.job_key) search.set('job_key', params.job_key)
  if (params?.state) search.set('state', params.state)
  if (params?.limit) search.set('limit', String(params.limit))
  const qs = search.toString() ? `?${search}` : ''
  return useQuery({
    queryKey: ['executions', params],
    queryFn: () => apiFetch<T.Execution[]>(`/v1/executions${qs}`),
    // High-frequency jobs (heartbeat = every minute) make even a 30 s
    // refetch feel stale. 5 s is cheap on a SQLite store with the
    // (state, fire_at) and created_at indexes added in #47.
    refetchInterval: 5_000,
  })
}
export function useExecutionLogs(executionId: string) {
  return useQuery({
    queryKey: ['execution-logs', executionId],
    queryFn: () => apiFetch<T.ExecutionLogEntry[]>(`/v1/executions/${executionId}/logs`),
    enabled: !!executionId,
  })
}

// Forecast
export function useForecast(windowMinutes = 120) {
  return useQuery({
    queryKey: ['forecast', windowMinutes],
    queryFn: () => apiFetch<T.ForecastResponse>(`/v1/dashboard/forecast?window_minutes=${windowMinutes}&bucket_minutes=5`),
    refetchInterval: 30_000,
  })
}

// Dead Letters
export function useDeadLetters(jobKey?: string) {
  const params = jobKey ? `?job_key=${jobKey}` : ''
  return useQuery({
    queryKey: ['dead-letters', jobKey],
    queryFn: () => apiFetch<T.DeadLetter[]>(`/v1/dead-letters${params}`),
    // Polling drives the header's bell badge counter — 10 s is a fair
    // compromise between freshness ("a job just died") and traffic.
    refetchInterval: 10_000,
  })
}
export function useDeadLetter(id: string) {
  return useQuery({
    queryKey: ['dead-letter', id],
    queryFn: () => apiFetch<T.DeadLetter>(`/v1/dead-letters/${id}`),
    enabled: !!id,
  })
}
export function useDeleteDeadLetter() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => apiDelete(`/v1/dead-letters/${id}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['dead-letters'] }),
    meta: { action: 'Delete dead letter' },
  })
}
export function useReplayDeadLetter() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => apiPost<T.ReplayResponse>(`/v1/dead-letters/${id}/replay`, {}),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['dead-letters'] }); qc.invalidateQueries({ queryKey: ['executions'] }) },
    meta: { action: 'Replay dead letter' },
  })
}

// Calendars
export function useCalendars() {
  return useQuery({ queryKey: ['calendars'], queryFn: () => apiFetch<T.CalendarDefinition[]>('/v1/calendars') })
}
export function useCreateCalendar() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: { name: string; timezone?: string; rules?: string }) =>
      apiPost<T.CalendarDefinition>('/v1/calendars', {
        name: data.name,
        timezone: data.timezone,
        rules: data.rules ?? '',
      }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['calendars'] }),
    meta: { action: 'Create calendar' },
  })
}
export function useUpdateCalendar() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({
      calendar_id,
      ...patch
    }: {
      calendar_id: string
      name?: string
      timezone?: string
      rules?: string
    }) => apiPut<T.CalendarDefinition>(`/v1/calendars/${calendar_id}`, patch),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['calendars'] }),
    meta: { action: 'Update calendar' },
  })
}
export function useDeleteCalendar() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => apiDelete(`/v1/calendars/${id}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['calendars'] }),
    meta: { action: 'Delete calendar' },
  })
}
/// POST /v1/calendars/{dsl-id}/adopt — copies a DSL calendar into the
/// API store (Phase 2). Requires `policy { dsl_adopt_on_mutate true }` in
/// the Croniqfile; otherwise the server returns 409.
export function useAdoptCalendar() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (dslId: string) =>
      apiPost<{ calendar: T.CalendarDefinition; dsl_key: string }>(
        `/v1/calendars/${dslId}/adopt`,
        {},
      ),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['calendars'] }),
    meta: { action: 'Adopt DSL calendar' },
  })
}
/// POST /v1/calendars/{api-id}/unadopt — drops an adopted API row so the
/// next reload reinstates the DSL definition.
export function useUnadoptCalendar() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (apiId: string) => apiPost<void>(`/v1/calendars/${apiId}/unadopt`, {}),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['calendars'] }),
    meta: { action: 'Unadopt calendar' },
  })
}
/// POST /v1/jobs/{job_key}/adopt — copies a DSL job + its trigger into
/// the API store. Same opt-in flag as adopt-calendar.
export function useAdoptJob() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (jobKey: string) =>
      apiPost<{ job: T.JobDefinition; trigger: unknown; dsl_key: string }>(
        `/v1/jobs/${jobKey}/adopt`,
        {},
      ),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['jobs'] })
      qc.invalidateQueries({ queryKey: ['schedules'] })
    },
    meta: { action: 'Adopt DSL job' },
  })
}
/// POST /v1/jobs/{job_key}/unadopt — drops the API copy + adoption record
/// so the next reload reinstates the DSL job definition + trigger.
export function useUnadoptJob() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (jobKey: string) => apiPost<void>(`/v1/jobs/${jobKey}/unadopt`, {}),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['jobs'] })
      qc.invalidateQueries({ queryKey: ['schedules'] })
    },
    meta: { action: 'Unadopt job' },
  })
}

// API Clients
export function useApiClients() {
  return useQuery({ queryKey: ['api-clients'], queryFn: () => apiFetch<T.ApiClient[]>('/v1/api-clients') })
}
export function useCreateApiClient() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: { name: string; scopes: string[] }) =>
      apiPost<T.CreateClientResponse>('/v1/api-clients', data),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['api-clients'] }),
    meta: { action: 'Create API client' },
  })
}
export function useUpdateApiClient() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({
      client_id,
      ...patch
    }: {
      client_id: string
      name?: string
      scopes?: string[]
      is_active?: boolean
    }) => apiPut<T.ApiClient>(`/v1/api-clients/${client_id}`, patch),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['api-clients'] }),
    meta: { action: 'Update API client' },
  })
}
export function useDeleteApiClient() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => apiDelete(`/v1/api-clients/${id}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['api-clients'] }),
    meta: { action: 'Delete API client' },
  })
}
export function useIssueClientToken() {
  return useMutation({
    mutationFn: (clientId: string) =>
      apiPost<T.CreateApiKeyResponse>('/v1/api-keys', { client_id: clientId }),
    meta: { action: 'Issue API key' },
  })
}
export function useRevokeApiKey() {
  return useMutation({
    mutationFn: (keyId: string) => apiDelete(`/v1/api-keys/${keyId}`),
    meta: { action: 'Revoke API key' },
  })
}
