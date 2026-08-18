import { useState, useEffect, useRef } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { apiFetch, apiPost, apiPut, apiDelete } from './client'
import type * as T from './types'
import { useAuthStore } from '@/auth/store'
import { refreshAccessToken } from '@/auth/session'

// Health
export function useHealth() {
  return useQuery({ queryKey: ['health'], queryFn: () => apiFetch<T.HealthResponse>('/health'), refetchInterval: 5000 })
}

// Version + environment metadata (public, no auth). Failures are
// expected against older backends that don't ship /version yet — the
// caller treats `null` data as "chip / badge stays hidden".
export function useVersion() {
  return useQuery({
    queryKey: ['version'],
    queryFn: async () => {
      try {
        return await apiFetch<T.VersionResponse>('/version')
      } catch {
        return null
      }
    },
    // The build never changes between renders; fetch once and pin.
    staleTime: Infinity,
    retry: false,
  })
}

// Global maintenance switch. Any authenticated user can read it (the banner
// polls this); only admins can set it. Poll ~10s so a scheduled window or an
// admin toggle appears/clears for everyone without a reload.
export function useMaintenance() {
  return useQuery({
    queryKey: ['maintenance'],
    queryFn: () => apiFetch<T.MaintenanceResponse>('/v1/maintenance'),
    refetchInterval: 10000,
  })
}

export function useSetMaintenance() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (body: {
      manual_active: boolean
      window_start: string | null
      window_end: string | null
      note: string | null
    }) => apiPut<T.MaintenanceResponse>('/v1/maintenance', body),
    onSuccess: (data) => qc.setQueryData(['maintenance'], data),
    meta: { action: 'Update maintenance mode' },
  })
}

// Jobs
export function useJobs() {
  return useQuery({ queryKey: ['jobs'], queryFn: () => apiFetch<T.JobDefinition[]>('/v1/jobs') })
}
export function useJob(jobKey: string) {
  return useQuery({ queryKey: ['jobs', jobKey], queryFn: () => apiFetch<T.JobDefinition>(`/v1/jobs/${jobKey}`) })
}
// Per-job scheduling liveness (issue #250). Polled so an "overdue" badge
// appears (and clears) without a manual refresh.
export function useJobStates() {
  return useQuery({
    queryKey: ['job-states'],
    queryFn: () => apiFetch<T.JobScheduleState[]>('/v1/jobs/states'),
    refetchInterval: 15000,
  })
}
export function useCreateJob() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: {
      job_key: string
      description?: string | null
      timeout?: string | null
      max_retries?: number | null
      dead_letter_enabled?: boolean | null
      dead_letter_retention?: string | null
      dead_letter_operator_hint?: string | null
      dead_letter_replay_max_age?: string | null
      tags?: string[]
    }) => apiPost<T.JobDefinition>('/v1/jobs', data),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['jobs'] })
      qc.invalidateQueries({ queryKey: ['tags'] })
    },
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
      dead_letter_retention?: string | null
      dead_letter_operator_hint?: string | null
      dead_letter_replay_max_age?: string | null
      tags?: string[]
    }) => apiPut<T.JobDefinition>(`/v1/jobs/${job_key}`, patch),
    onSuccess: (_data, vars) => {
      qc.invalidateQueries({ queryKey: ['jobs'] })
      qc.invalidateQueries({ queryKey: ['jobs', vars.job_key] })
      qc.invalidateQueries({ queryKey: ['tags'] })
    },
    meta: { action: 'Update job' },
  })
}

// Tags
export function useJobTags() {
  return useQuery({
    queryKey: ['tags', 'jobs'],
    queryFn: () => apiFetch<T.TagCount[]>('/v1/tags?entity=jobs'),
  })
}

export function useRunnerTags() {
  return useQuery({
    queryKey: ['tags', 'runners'],
    queryFn: () => apiFetch<T.TagCount[]>('/v1/tags?entity=runners'),
    refetchInterval: 10000,
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
      window?: string
      enabled?: boolean
    }) => apiPost<T.TriggerDefinition>('/v1/schedules', data),
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
      timezone?: string | null
      calendar?: string | null
      window?: string | null
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
        if (res.status === 401) {
          // An expired access token, most likely: the stream outlives it by
          // design. Refresh and reconnect rather than ending the session
          // (issue #454); a genuinely dead session fails the refresh and
          // `refreshAccessToken` clears it for us.
          if (await refreshAccessToken()) throw new Error('SSE 401 — retrying with a fresh token')
          return
        }
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
export function useExecutions(params?: { job_key?: string; state?: string; limit?: number; runner_id?: string }) {
  const search = new URLSearchParams()
  if (params?.job_key) search.set('job_key', params.job_key)
  if (params?.state) search.set('state', params.state)
  if (params?.limit) search.set('limit', String(params.limit))
  if (params?.runner_id) search.set('runner_id', params.runner_id)
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
export function useCancelExecution() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (executionId: string) =>
      apiFetch<{ execution_id: string; cancelled: boolean; delivered_via_runner: boolean }>(
        `/v1/executions/${executionId}/cancel`,
        { method: 'POST' },
      ),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['executions'] })
    },
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
// Bulk delete: either an explicit `ids` list, or `all: true` (optionally
// scoped to a `job_key`) to clear the queue. Returns `{ deleted }`.
export function useBulkDeleteDeadLetters() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (body: { ids?: string[]; all?: boolean; job_key?: string }) =>
      apiPost<T.BulkDeleteResponse>('/v1/dead-letters/bulk-delete', body),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['dead-letters'] }),
    meta: { action: 'Delete dead letters' },
  })
}
export function useReplayDeadLetter() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ id, force }: { id: string; force?: boolean }) =>
      apiPost<T.ReplayResponse>(`/v1/dead-letters/${id}/replay`, force ? { force: true } : {}),
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

// PR-B1 stats + audit ─────────────────────────────────────────────
export function useAuditEvents(params?: {
  limit?: number
  actor_id?: string
  target_type?: string
  action?: string
}) {
  const search = new URLSearchParams()
  if (params?.limit) search.set('limit', String(params.limit))
  if (params?.actor_id) search.set('actor_id', params.actor_id)
  if (params?.target_type) search.set('target_type', params.target_type)
  if (params?.action) search.set('action', params.action)
  const qs = search.toString()
  return useQuery({
    queryKey: ['audit', params ?? {}],
    queryFn: () => apiFetch<T.AuditEvent[]>(`/v1/audit${qs ? `?${qs}` : ''}`),
    staleTime: 30_000,
  })
}

export function useJobStats(jobKey: string, days = 7) {
  return useQuery({
    queryKey: ['jobs', jobKey, 'stats', days],
    enabled: !!jobKey,
    queryFn: () =>
      apiFetch<T.JobStatsResponse>(`/v1/jobs/${encodeURIComponent(jobKey)}/stats?days=${days}`),
    staleTime: 60_000,
  })
}

export function useThroughput(window: '1h' | '6h' | '24h' | '7d' = '24h') {
  return useQuery({
    queryKey: ['executions', 'throughput', window],
    queryFn: () => apiFetch<T.ThroughputResponse>(`/v1/executions/throughput?window=${window}`),
    refetchInterval: 30_000,
    staleTime: 30_000,
  })
}

export function useFailureHeatmap(days = 28) {
  return useQuery({
    queryKey: ['insights', 'failures', days],
    queryFn: () => apiFetch<T.FailureHeatmap>(`/v1/insights/failures?days=${days}`),
    staleTime: 60_000,
  })
}

// Users (admin)
export function useUsers() {
  return useQuery({
    queryKey: ['users'],
    queryFn: () => apiFetch<T.User[]>('/v1/users'),
    staleTime: 30_000,
  })
}
export function useDeleteUser() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (userId: string) => apiDelete(`/v1/users/${userId}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['users'] }),
    meta: { action: 'Delete user' },
  })
}

// Invitations (admin)
export function useInvitations() {
  return useQuery({
    queryKey: ['invitations'],
    queryFn: () => apiFetch<T.Invitation[]>('/v1/invitations'),
    staleTime: 30_000,
  })
}
export function useCreateInvitation() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (body: { email: string; role: T.Role; expires_in_hours?: number }) =>
      apiPost<T.CreateInvitationResponse>('/v1/invitations', body),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['invitations'] }),
    meta: { action: 'Create invitation' },
  })
}
export function useRevokeInvitation() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => apiDelete(`/v1/invitations/${id}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['invitations'] }),
    meta: { action: 'Revoke invitation' },
  })
}

// Personal Access Tokens (self)
export function usePersonalAccessTokens() {
  return useQuery({
    queryKey: ['users', 'me', 'tokens'],
    queryFn: () => apiFetch<T.PersonalAccessToken[]>('/v1/users/me/tokens'),
    staleTime: 30_000,
  })
}
export function useCreatePat() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (body: { name: string; scopes: string[]; expires_in_hours?: number }) =>
      apiPost<T.CreatePatResponse>('/v1/users/me/tokens', body),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['users', 'me', 'tokens'] }),
    meta: { action: 'Create PAT' },
  })
}
export function useRevokePat() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => apiDelete(`/v1/users/me/tokens/${id}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['users', 'me', 'tokens'] }),
    meta: { action: 'Revoke PAT' },
  })
}

// TOTP (self) — setup returns the secret + recovery codes, confirm enables it.
export function useTotpSetup() {
  return useMutation({
    mutationFn: () => apiPost<T.TotpSetupResponse>('/v1/users/me/totp/setup', {}),
    meta: { action: 'Begin TOTP setup' },
  })
}
export function useTotpConfirm() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (code: string) => apiPost('/v1/users/me/totp/confirm', { code }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['users', 'me'] }),
    meta: { action: 'Confirm TOTP' },
  })
}
export function useTotpDisable() {
  const qc = useQueryClient()
  return useMutation({
    // The server requires a fresh password proof to disable 2FA, not a
    // current TOTP code — disabling a second factor is a security
    // downgrade, so it re-verifies the primary credential.
    mutationFn: (password: string) => apiPost('/v1/users/me/totp/disable', { password }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['users', 'me'] }),
    meta: { action: 'Disable TOTP' },
  })
}

// Current user — only resolves for password/OIDC/PAT logins, not anonymous
// API-key sessions. Returns 404 for callers without a user record; the hook
// surfaces that as `data === null` so the UI can branch cleanly.
export function useCurrentUser() {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['users', 'me'],
    enabled: !!token,
    queryFn: () =>
      apiFetch<T.User>('/v1/users/me').catch((err) => {
        if (err instanceof Error && err.message.toLowerCase().includes('not found')) return null
        throw err
      }),
    staleTime: 60_000,
  })
}

// ─── #140 Failure alerts ─────────────────────────────────────────

/// `GET /v1/alerts/config` — the effective alerts config the server
/// is running with. Channels + rules are DSL-managed today, so this
/// is read-only and stays stable across renders (cached 60 s).
export function useAlertsConfig() {
  return useQuery({
    queryKey: ['alerts', 'config'],
    queryFn: () => apiFetch<T.AlertsConfig>('/v1/alerts/config'),
    staleTime: 60_000,
    refetchOnWindowFocus: false,
  })
}

/// `GET /v1/alerts/deliveries` with optional filters. Polled every
/// 15 s so an operator watching the page sees new fires arrive
/// without manual refresh.
export function useAlertDeliveries(filter: T.AlertDeliveryListQuery = {}) {
  const params = new URLSearchParams()
  if (filter.job_key) params.set('job_key', filter.job_key)
  if (filter.rule_name) params.set('rule_name', filter.rule_name)
  if (filter.state) params.set('state', filter.state)
  if (filter.since) params.set('since', filter.since)
  if (filter.limit != null) params.set('limit', String(filter.limit))
  const qs = params.toString() ? `?${params.toString()}` : ''
  return useQuery({
    queryKey: ['alerts', 'deliveries', filter],
    queryFn: () => apiFetch<T.AlertDelivery[]>(`/v1/alerts/deliveries${qs}`),
    refetchInterval: 15_000,
  })
}

/// `GET /v1/alerts/deliveries/{id}` — single-row lookup for a
/// detail pane / share-link.
export function useAlertDelivery(id: string) {
  return useQuery({
    queryKey: ['alerts', 'delivery', id],
    queryFn: () => apiFetch<T.AlertDelivery>(`/v1/alerts/deliveries/${id}`),
    enabled: !!id,
  })
}

// ─── #231 Alert rule overrides (admin-only `alerts:write`) ────────
// Each set-action overwrites the rule's override wholesale (snooze |
// disable | throttle are distinct intents, not composable) and returns
// the persisted row. All four invalidate the config query so the inline
// override view refreshes.

/// `POST /v1/alerts/rules/{name}/snooze` — suppress until `until`,
/// which doubles as the auto-clear deadline. `note` is mandatory.
export function useSnoozeRule() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ name, until, note }: { name: string; until: string; note: string }) =>
      apiPost<T.AlertRuleOverride>(
        `/v1/alerts/rules/${encodeURIComponent(name)}/snooze`,
        { until, note },
      ),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['alerts', 'config'] }),
    meta: { action: 'Snooze alert rule' },
  })
}

/// `POST /v1/alerts/rules/{name}/disable` — disable the rule;
/// `expires_at` optionally auto-re-enables (omit for open-ended).
export function useDisableRule() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({
      name,
      note,
      expires_at,
    }: {
      name: string
      note: string
      expires_at?: string | null
    }) =>
      apiPost<T.AlertRuleOverride>(
        `/v1/alerts/rules/${encodeURIComponent(name)}/disable`,
        { note, expires_at: expires_at ?? null },
      ),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['alerts', 'config'] }),
    meta: { action: 'Disable alert rule' },
  })
}

/// `POST /v1/alerts/rules/{name}/throttle` — replace the DSL throttle
/// window with `throttle` (a duration string like `"30m"`).
export function useThrottleRule() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({
      name,
      throttle,
      note,
      expires_at,
    }: {
      name: string
      throttle: string
      note: string
      expires_at?: string | null
    }) =>
      apiPost<T.AlertRuleOverride>(
        `/v1/alerts/rules/${encodeURIComponent(name)}/throttle`,
        { throttle, note, expires_at: expires_at ?? null },
      ),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['alerts', 'config'] }),
    meta: { action: 'Throttle alert rule' },
  })
}

/// `DELETE /v1/alerts/rules/{name}/override` — clear the override,
/// returning the rule to pure DSL behaviour.
export function useClearOverride() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (name: string) =>
      apiDelete(`/v1/alerts/rules/${encodeURIComponent(name)}/override`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['alerts', 'config'] }),
    meta: { action: 'Clear alert override' },
  })
}
