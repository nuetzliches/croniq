import { useMutation, useQuery, useQueryClient } from '@tanstack/vue-query'
import { computed, type MaybeRefOrGetter, toValue } from 'vue'
import { api, apiDelete, apiGet, apiPost, apiPut } from './client'
import type {
  AlertDelivery,
  AlertDeliveryListQuery,
  AlertRuleOverride,
  AlertsConfig,
  AuthConfigResponse,
  CalendarDefinition,
  DeadLetter,
  Execution,
  ExecutionLogEntry,
  FailureHeatmap,
  ForecastResponse,
  HealthResponse,
  JobDefinition,
  JobScheduleState,
  JobStatsResponse,
  MaintenanceResponse,
  ReloadSuccess,
  RunnerSummary,
  ThroughputResponse,
  TriggerDefinition,
  TriggerResponse,
  User,
  VersionResponse,
} from './types'
import { useAuthStore } from '~/stores/auth'

/**
 * Queries the shell needs. Screen-specific ones live with their screens; these
 * are the three things the frame itself asks for.
 */

/**
 * The signed-in user.
 *
 * Gated on `isAuthenticated` rather than fired unconditionally: before the
 * bootstrap refresh answers there is no access token, and an unconditional
 * query would spend a guaranteed 401 on every page load. `enabled` takes a ref
 * so it re-evaluates when the store settles — passing `auth.isAuthenticated`
 * by value would freeze it at `false` forever, which is the single most common
 * way to misuse vue-query.
 */
export function useCurrentUser() {
  const auth = useAuthStore()
  return useQuery({
    queryKey: ['users', 'me'],
    queryFn: () => apiGet<User>('/v1/users/me'),
    enabled: computed(() => auth.isAuthenticated),
    staleTime: 60_000,
  })
}

/**
 * Which login methods the server offers. Public, so it works signed out —
 * which is the point, the login page needs it before any auth happens.
 */
export function useAuthConfig() {
  return useQuery({
    queryKey: ['auth', 'config'],
    queryFn: () => apiGet<AuthConfigResponse>('/v1/auth/config'),
    staleTime: Infinity,
    retry: false,
  })
}

/**
 * Count for the Dead Letters badge.
 *
 * A count, not the list: the badge needs a number and the screen fetches its
 * own rows. Polled because a dead letter appearing is exactly the kind of thing
 * an operator should not have to reload to notice.
 */
export function useDeadLetterCount() {
  const auth = useAuthStore()
  const query = useQuery({
    queryKey: ['dead-letters', 'count'],
    queryFn: () => apiGet<DeadLetter[]>('/v1/dead-letters', { limit: 100 }),
    enabled: computed(() => auth.isAuthenticated),
    refetchInterval: 30_000,
  })
  return computed(() => query.data.value?.length ?? 0)
}

/**
 * Server health. Public, so the login page can show it before anyone signs in
 * — which is where the shipping dashboard uses it, and a genuinely good idea:
 * you learn the server is alive before you have credentials to check with.
 *
 * Polled, because the shell's live dot is only worth having if it can go out.
 */
export function useHealth() {
  return useQuery({
    queryKey: ['health'],
    queryFn: () => apiGet<HealthResponse>('/health'),
    refetchInterval: 5_000,
    retry: false,
  })
}

/**
 * Build version. Public, and pinned forever — it cannot change without the
 * process restarting, at which point the page reloads anyway.
 *
 * Failures are expected against an older server that has no `/version`, so the
 * caller treats `undefined` as "hide the chip" rather than as an error.
 */
export function useVersion() {
  return useQuery({
    queryKey: ['version'],
    queryFn: () => apiGet<VersionResponse>('/version'),
    staleTime: Infinity,
    retry: false,
  })
}

/**
 * The global maintenance switch.
 *
 * Any authenticated user may read it — the banner is for everyone, since
 * maintenance pauses dispatch and a dashboard that looks normal while nothing
 * fires is misleading. Only admins may set it.
 *
 * Polled at ten seconds so a window opening or an admin toggling it reaches
 * every open tab without a reload.
 */
export function useMaintenance() {
  const auth = useAuthStore()
  return useQuery({
    queryKey: ['maintenance'],
    queryFn: () => apiGet<MaintenanceResponse>('/v1/maintenance'),
    enabled: computed(() => auth.isAuthenticated),
    refetchInterval: 10_000,
  })
}

export interface MaintenancePatch {
  manual_active: boolean
  window_start: string | null
  window_end: string | null
  note: string | null
}

/**
 * Set the maintenance switch. Admin-only, enforced by the server.
 *
 * The response is written straight into the cache rather than invalidated: the
 * banner is driven by the same query, and a round trip between clicking "Save"
 * and the banner appearing reads as the action not having worked.
 */
export function useSetMaintenance() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (patch: MaintenancePatch) =>
      apiPut<MaintenanceResponse>('/v1/maintenance', patch),
    onSuccess: (data) => queryClient.setQueryData(['maintenance'], data),
  })
}

/**
 * Re-read the Croniqfile.
 *
 * `dryRun` is what makes this safe to put behind a button: the server
 * validates, computes the diff and reports the boot-only settings that would
 * stay pending — all without touching anything. The dashboard shows that, and
 * only then offers to apply.
 *
 * Not invalidating job queries on a dry run is deliberate; nothing changed.
 * On a real apply the schedule did change, so everything that reads it is
 * dropped.
 */
export function useReloadConfig() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ dryRun }: { dryRun: boolean }) =>
      api<ReloadSuccess>(`/v1/admin/reload-config${dryRun ? '?dry_run=true' : ''}`, {
        method: 'POST',
      }),
    onSuccess: (result) => {
      if (result.applied) {
        for (const key of [['jobs'], ['schedules'], ['calendars'], ['job-states']]) {
          void queryClient.invalidateQueries({ queryKey: key })
        }
      }
    },
  })
}

export interface ExecutionFilters {
  job_key?: string
  state?: string
  runner_id?: string
  limit?: number
}

/**
 * Runs, filtered.
 *
 * **The parameter is a getter, not a value, and that is load-bearing.** This is
 * the single most-named risk in `docs/vue-migration-plan.md`: pass a plain
 * object here and both the query key and the request freeze at whatever the
 * filters were on first render, so changing a filter re-renders the page and
 * silently shows the old rows. `toValue` inside `queryKey` and `queryFn` is
 * what makes the query re-run.
 *
 * Polled: a run list that does not move is indistinguishable from a scheduler
 * that has stopped.
 */
export function useExecutions(filters: MaybeRefOrGetter<ExecutionFilters>) {
  const auth = useAuthStore()
  return useQuery({
    queryKey: ['executions', computed(() => toValue(filters))],
    queryFn: () => {
      const active = toValue(filters)
      const query: Record<string, string | number> = {}
      if (active.job_key) query.job_key = active.job_key
      if (active.state) query.state = active.state
      if (active.runner_id) query.runner_id = active.runner_id
      query.limit = active.limit ?? 200
      return apiGet<Execution[]>('/v1/executions', query)
    },
    enabled: computed(() => auth.isAuthenticated),
    refetchInterval: 5_000,
    // Keep the previous rows on screen while a filter change is in flight, so
    // the table does not blink through an empty state on every keystroke.
    placeholderData: (previous) => previous,
  })
}

/**
 * The log events one run produced.
 *
 * There is deliberately no `useExecution`: the server has no
 * `GET /v1/executions/{id}` — only the list, `/cancel` and `/logs` — so the
 * detail panel resolves its row out of the list it already has, which is what
 * the React tree does too. Inventing a fetch for it would have meant inventing
 * an endpoint.
 */
export function useExecutionLogs(id: MaybeRefOrGetter<string | undefined>) {
  const auth = useAuthStore()
  return useQuery({
    queryKey: ['executions', 'logs', computed(() => toValue(id))],
    queryFn: () => apiGet<ExecutionLogEntry[]>(`/v1/executions/${toValue(id)}/logs`),
    enabled: computed(() => auth.isAuthenticated && Boolean(toValue(id))),
    refetchInterval: 5_000,
  })
}

/**
 * Ask a running execution to stop.
 *
 * The server answers whether it reached a runner: a queued run is cancelled
 * outright, a claimed one depends on the runner honouring the signal, so the
 * caller has something honest to report either way.
 */
export function useCancelExecution() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      api<{ execution_id: string; cancelled: boolean; delivered_via_runner: boolean }>(
        `/v1/executions/${id}/cancel`,
        { method: 'POST' },
      ),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['executions'] }),
  })
}

/**
 * What is about to happen.
 *
 * The endpoint has existed all along and the React dashboard never called it —
 * only the job detail did. The audit's sharpest finding was that croniq shows
 * the past on every screen and the future on none, which turned out to be a
 * missing call rather than missing data.
 *
 * Polled at a minute: the window slides, so a rail that never refreshes drifts
 * into showing fires that have already happened.
 */
export function useForecast(windowMinutes = 60, bucketMinutes = 5) {
  const auth = useAuthStore()
  return useQuery({
    queryKey: ['forecast', windowMinutes, bucketMinutes],
    queryFn: () =>
      apiGet<ForecastResponse>('/v1/dashboard/forecast', {
        window_minutes: windowMinutes,
        bucket_minutes: bucketMinutes,
      }),
    enabled: computed(() => auth.isAuthenticated),
    refetchInterval: 60_000,
  })
}

/** Per-job scheduling liveness: next fire, last fire, and whether it is late. */
export function useJobStates() {
  const auth = useAuthStore()
  return useQuery({
    queryKey: ['job-states'],
    queryFn: () => apiGet<JobScheduleState[]>('/v1/jobs/states'),
    enabled: computed(() => auth.isAuthenticated),
    refetchInterval: 15_000,
  })
}

export function useJobs() {
  const auth = useAuthStore()
  return useQuery({
    queryKey: ['jobs'],
    queryFn: () => apiGet<JobDefinition[]>('/v1/jobs'),
    enabled: computed(() => auth.isAuthenticated),
  })
}

export function useThroughput(window = '24h') {
  const auth = useAuthStore()
  return useQuery({
    queryKey: ['throughput', window],
    queryFn: () => apiGet<ThroughputResponse>('/v1/executions/throughput', { window }),
    enabled: computed(() => auth.isAuthenticated),
    refetchInterval: 60_000,
  })
}

export function useFailureHeatmap(days = 7) {
  const auth = useAuthStore()
  return useQuery({
    queryKey: ['failure-heatmap', days],
    queryFn: () => apiGet<FailureHeatmap>('/v1/insights/failures', { days }),
    enabled: computed(() => auth.isAuthenticated),
    refetchInterval: 60_000,
  })
}

export function useRunners() {
  const auth = useAuthStore()
  return useQuery({
    queryKey: ['runners'],
    queryFn: () => apiGet<RunnerSummary[]>('/v1/runners'),
    enabled: computed(() => auth.isAuthenticated),
    refetchInterval: 10_000,
  })
}

export function useDeadLetters(jobKey?: MaybeRefOrGetter<string | undefined>) {
  const auth = useAuthStore()
  return useQuery({
    queryKey: ['dead-letters', computed(() => (jobKey ? toValue(jobKey) : undefined))],
    queryFn: () => {
      const key = jobKey ? toValue(jobKey) : undefined
      return apiGet<DeadLetter[]>('/v1/dead-letters', key ? { job_key: key } : undefined)
    },
    enabled: computed(() => auth.isAuthenticated),
    refetchInterval: 30_000,
  })
}

/**
 * Put a dead letter back in the queue.
 *
 * The server refuses one whose logical fire time is too old for the job's
 * policy, so a caller must be ready for a 4xx that is a decision rather than a
 * fault — see the replay guard in operations.md.
 */
export function useReplayDeadLetter() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api(`/v1/dead-letters/${id}/replay`, { method: 'POST' }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['dead-letters'] })
      void queryClient.invalidateQueries({ queryKey: ['executions'] })
    },
  })
}

export function useDeleteDeadLetter() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api(`/v1/dead-letters/${id}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['dead-letters'] }),
  })
}

export function useDeleteRunner() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api(`/v1/runners/${id}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['runners'] }),
  })
}

/* ─── Jobs ────────────────────────────────────────────────────────────────
 *
 * The job screen's data comes from four endpoints that the React tree never
 * joined: the definition (`/v1/jobs`), the scheduling liveness
 * (`/v1/jobs/states`), the triggers (`/v1/schedules`) and the per-job
 * statistics. Joining them is what lets the list answer "what fires next and
 * is anything late" without opening a job.
 * ─────────────────────────────────────────────────────────────────────────── */

/** One job. The list is the source for the detail; this is for a deep link. */
export function useJob(jobKey: MaybeRefOrGetter<string | undefined>) {
  const auth = useAuthStore()
  const key = computed(() => toValue(jobKey))
  return useQuery({
    queryKey: ['jobs', key],
    queryFn: () => apiGet<JobDefinition>(`/v1/jobs/${encodeURIComponent(key.value!)}`),
    enabled: computed(() => auth.isAuthenticated && Boolean(key.value)),
  })
}

/**
 * Triggers, optionally for one job.
 *
 * A getter, not a value — the same trap `useExecutions` documents. The job
 * detail changes its key by navigation, and a frozen query key would leave the
 * previous job's schedule on screen.
 */
export function useSchedules(jobKey?: MaybeRefOrGetter<string | undefined>) {
  const auth = useAuthStore()
  const key = computed(() => (jobKey ? toValue(jobKey) : undefined))
  return useQuery({
    queryKey: ['schedules', key],
    queryFn: () =>
      apiGet<TriggerDefinition[]>('/v1/schedules', key.value ? { job_key: key.value } : undefined),
    enabled: computed(() => auth.isAuthenticated),
  })
}

/** Success rate and latency percentiles over a window, for one job. */
export function useJobStats(jobKey: MaybeRefOrGetter<string | undefined>, days = 7) {
  const auth = useAuthStore()
  const key = computed(() => toValue(jobKey))
  return useQuery({
    queryKey: ['job-stats', key, days],
    queryFn: () =>
      apiGet<JobStatsResponse>(`/v1/jobs/${encodeURIComponent(key.value!)}/stats`, { days }),
    enabled: computed(() => auth.isAuthenticated && Boolean(key.value)),
  })
}

/** Calendars, for the schedule editor's binding and for the calendars screen. */
export function useCalendars() {
  const auth = useAuthStore()
  return useQuery({
    queryKey: ['calendars'],
    queryFn: () => apiGet<CalendarDefinition[]>('/v1/calendars'),
    enabled: computed(() => auth.isAuthenticated),
  })
}

/**
 * Everything a job mutation touches.
 *
 * Collected in one place because the list joins four queries: changing a job's
 * schedule moves its next fire time, which is a column in the list and a number
 * on the dashboard. Invalidating only `['jobs']` would leave both stale, and
 * that staleness looks exactly like the mutation not having worked.
 */
function invalidateJob(queryClient: ReturnType<typeof useQueryClient>) {
  for (const key of [['jobs'], ['job-states'], ['schedules'], ['job-stats'], ['forecast']]) {
    void queryClient.invalidateQueries({ queryKey: key })
  }
}

export interface JobPatch {
  description?: string | null
  timeout?: string | null
  max_retries?: number | null
  dead_letter_enabled?: boolean | null
  dead_letter_retention?: string | null
  dead_letter_operator_hint?: string | null
  dead_letter_replay_max_age?: string | null
  tags?: string[]
}

export function useCreateJob() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (data: JobPatch & { job_key: string }) =>
      apiPost<JobDefinition>('/v1/jobs', data),
    onSuccess: () => invalidateJob(queryClient),
  })
}

export function useUpdateJob() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ job_key, ...patch }: JobPatch & { job_key: string }) =>
      apiPut<JobDefinition>(`/v1/jobs/${encodeURIComponent(job_key)}`, patch),
    onSuccess: () => invalidateJob(queryClient),
  })
}

export function useDeleteJob() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (jobKey: string) => apiDelete(`/v1/jobs/${encodeURIComponent(jobKey)}`),
    onSuccess: () => invalidateJob(queryClient),
  })
}

/**
 * Pause and resume.
 *
 * Two endpoints behind one hook, because from the operator's side it is one
 * switch and calling it that way keeps the caller from having to hold two
 * mutation objects to render one control.
 */
export function useSetJobActive() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ jobKey, active }: { jobKey: string; active: boolean }) =>
      apiPost<JobDefinition>(
        `/v1/jobs/${encodeURIComponent(jobKey)}/${active ? 'activate' : 'deactivate'}`,
        {},
      ),
    onSuccess: () => invalidateJob(queryClient),
  })
}

/**
 * Fire a job now.
 *
 * The response carries `deduplicated`: the server coalesced this trigger into
 * an execution that was already queued under the same idempotency key (#279).
 * That is not a failure and not a success either — the caller reports it,
 * because "nothing happened" and "it joined an existing run" look identical in
 * the run list.
 */
export function useTriggerJob() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (jobKey: string) => apiPost<TriggerResponse>('/v1/trigger', { job_key: jobKey }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['executions'] })
      void queryClient.invalidateQueries({ queryKey: ['job-states'] })
    },
  })
}

/**
 * Adoption — the DSL/API boundary.
 *
 * A job declared in the Croniqfile is read-only through the API: the next
 * reload would overwrite anything written here. Adopting copies the job and
 * its trigger into the API store, where they can be edited, and the Croniqfile
 * definition is ignored until it is unadopted again. It requires
 * `policy { dsl_adopt_on_mutate true }` on the server, so the refusal is a
 * configuration answer and worth showing verbatim.
 */
export function useAdoptJob() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (jobKey: string) =>
      apiPost<{ job: JobDefinition; dsl_key: string }>(
        `/v1/jobs/${encodeURIComponent(jobKey)}/adopt`,
        {},
      ),
    onSuccess: () => invalidateJob(queryClient),
  })
}

export function useUnadoptJob() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (jobKey: string) =>
      apiPost<void>(`/v1/jobs/${encodeURIComponent(jobKey)}/unadopt`, {}),
    onSuccess: () => invalidateJob(queryClient),
  })
}

export interface SchedulePatch {
  cron_expression?: string
  timezone?: string | null
  calendar?: string | null
  window?: string | null
  enabled?: boolean
}

export function useCreateSchedule() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (data: SchedulePatch & { job_key: string; cron_expression: string }) =>
      apiPost<TriggerDefinition>('/v1/schedules', data),
    onSuccess: () => invalidateJob(queryClient),
  })
}

export function useUpdateSchedule() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ trigger_id, ...patch }: SchedulePatch & { trigger_id: string }) =>
      apiPut<TriggerDefinition>(`/v1/schedules/${encodeURIComponent(trigger_id)}`, patch),
    onSuccess: () => invalidateJob(queryClient),
  })
}

export function useDeleteSchedule() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => apiDelete(`/v1/schedules/${encodeURIComponent(id)}`),
    onSuccess: () => invalidateJob(queryClient),
  })
}

/* ─── Calendars ───────────────────────────────────────────────────────────
 *
 * `useCalendars` lives above, with the job queries — the schedule editor
 * needed it first. These are the mutations.
 * ─────────────────────────────────────────────────────────────────────────── */

/**
 * A calendar change can move every job gated by it, so the invalidation
 * reaches further than `['calendars']`: next fire times, the forecast and the
 * job list all read through the gate.
 */
function invalidateCalendar(queryClient: ReturnType<typeof useQueryClient>) {
  for (const key of [['calendars'], ['job-states'], ['schedules'], ['forecast']]) {
    void queryClient.invalidateQueries({ queryKey: key })
  }
}

export interface CalendarPatch {
  name?: string
  timezone?: string
  rules?: string
}

export function useCreateCalendar() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (data: CalendarPatch & { name: string }) =>
      apiPost<CalendarDefinition>('/v1/calendars', { rules: '', ...data }),
    onSuccess: () => invalidateCalendar(queryClient),
  })
}

export function useUpdateCalendar() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ calendar_id, ...patch }: CalendarPatch & { calendar_id: string }) =>
      apiPut<CalendarDefinition>(`/v1/calendars/${encodeURIComponent(calendar_id)}`, patch),
    onSuccess: () => invalidateCalendar(queryClient),
  })
}

export function useDeleteCalendar() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => apiDelete(`/v1/calendars/${encodeURIComponent(id)}`),
    onSuccess: () => invalidateCalendar(queryClient),
  })
}

/** Copy a Croniqfile calendar into the API store so it can be edited. */
export function useAdoptCalendar() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (dslId: string) =>
      apiPost<{ calendar: CalendarDefinition; dsl_key: string }>(
        `/v1/calendars/${encodeURIComponent(dslId)}/adopt`,
        {},
      ),
    onSuccess: () => invalidateCalendar(queryClient),
  })
}

export function useUnadoptCalendar() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (apiId: string) =>
      apiPost<void>(`/v1/calendars/${encodeURIComponent(apiId)}/unadopt`, {}),
    onSuccess: () => invalidateCalendar(queryClient),
  })
}

/* ─── Alerts ──────────────────────────────────────────────────────────────
 *
 * Two shapes that answer each other: the configuration (channels and rules,
 * all of it declared in the Croniqfile and read-only here) and the delivery
 * log (what actually went out). The one thing that *is* writable is an
 * override — snooze, disable, throttle — which is an operational decision
 * taken because of what the log shows.
 * ─────────────────────────────────────────────────────────────────────────── */

/** Channels, rules and any active overrides, as the server resolved them. */
export function useAlertsConfig() {
  const auth = useAuthStore()
  return useQuery({
    queryKey: ['alerts', 'config'],
    queryFn: () => apiGet<AlertsConfig>('/v1/alerts/config'),
    enabled: computed(() => auth.isAuthenticated),
  })
}

/**
 * The delivery log.
 *
 * A getter, like `useExecutions` — the filters live in the URL and change
 * under the query. Polled rather than streamed: deliveries are rare compared
 * to runs, and an SSE surface for them would be a third stream to keep alive
 * for a list that changes a few times an hour.
 */
export function useAlertDeliveries(filters: MaybeRefOrGetter<AlertDeliveryListQuery>) {
  const auth = useAuthStore()
  const active = computed(() => toValue(filters))
  return useQuery({
    queryKey: ['alerts', 'deliveries', active],
    queryFn: () => {
      const query: Record<string, string | number> = {}
      const { job_key, rule_name, state, since, limit } = active.value
      if (job_key) query.job_key = job_key
      if (rule_name) query.rule_name = rule_name
      if (state) query.state = state
      if (since) query.since = since
      query.limit = limit ?? 200
      return apiGet<AlertDelivery[]>('/v1/alerts/deliveries', query)
    },
    enabled: computed(() => auth.isAuthenticated),
    refetchInterval: 20_000,
  })
}

/**
 * Overrides.
 *
 * Snooze, disable and throttle are three distinct intents and the server
 * treats them as such: each call replaces the rule's override wholesale rather
 * than merging into it. One hook per intent keeps that visible at the call
 * site instead of hiding it behind a single `setOverride` that would imply
 * they compose.
 */
function invalidateAlerts(queryClient: ReturnType<typeof useQueryClient>) {
  void queryClient.invalidateQueries({ queryKey: ['alerts'] })
}

export function useSnoozeRule() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ name, until, note }: { name: string; until: string; note: string }) =>
      apiPost<AlertRuleOverride>(`/v1/alerts/rules/${encodeURIComponent(name)}/snooze`, {
        until,
        note,
      }),
    onSuccess: () => invalidateAlerts(queryClient),
  })
}

export function useDisableRule() {
  const queryClient = useQueryClient()
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
      apiPost<AlertRuleOverride>(`/v1/alerts/rules/${encodeURIComponent(name)}/disable`, {
        note,
        expires_at: expires_at ?? null,
      }),
    onSuccess: () => invalidateAlerts(queryClient),
  })
}

export function useThrottleRule() {
  const queryClient = useQueryClient()
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
      apiPost<AlertRuleOverride>(`/v1/alerts/rules/${encodeURIComponent(name)}/throttle`, {
        throttle,
        note,
        expires_at: expires_at ?? null,
      }),
    onSuccess: () => invalidateAlerts(queryClient),
  })
}

/** Back to whatever the Croniqfile says. */
export function useClearOverride() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) =>
      apiDelete(`/v1/alerts/rules/${encodeURIComponent(name)}/override`),
    onSuccess: () => invalidateAlerts(queryClient),
  })
}
