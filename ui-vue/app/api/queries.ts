import { useMutation, useQuery, useQueryClient } from '@tanstack/vue-query'
import { computed, type MaybeRefOrGetter, toValue } from 'vue'
import { api, apiGet, apiPut } from './client'
import type {
  AuthConfigResponse,
  DeadLetter,
  HealthResponse,
  Execution,
  ExecutionLogEntry,
  MaintenanceResponse,
  ReloadSuccess,
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
