import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { apiFetch, apiPost, apiDelete } from './client'
import type * as T from './types'

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
  return useMutation({ mutationFn: (data: { job_key: string; description?: string }) => apiPost('/v1/jobs', data), onSuccess: () => qc.invalidateQueries({ queryKey: ['jobs'] }) })
}
export function useDeleteJob() {
  const qc = useQueryClient()
  return useMutation({ mutationFn: (jobKey: string) => apiDelete(`/v1/jobs/${jobKey}`), onSuccess: () => qc.invalidateQueries({ queryKey: ['jobs'] }) })
}
export function useRegisterJob() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: { job_key: string; schedule: string; timezone?: string; timeout?: string; description?: string }) =>
      apiPost('/v1/jobs/register', data),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['jobs'] }); qc.invalidateQueries({ queryKey: ['schedules'] }) },
  })
}

// Schedules
export function useSchedules(jobKey?: string) {
  const params = jobKey ? `?job_key=${jobKey}` : ''
  return useQuery({ queryKey: ['schedules', jobKey], queryFn: () => apiFetch<T.TriggerDefinition[]>(`/v1/schedules${params}`) })
}
export function useCreateSchedule() {
  const qc = useQueryClient()
  return useMutation({ mutationFn: (data: { job_key: string; cron_expression: string; timezone?: string }) => apiPost('/v1/schedules', data), onSuccess: () => qc.invalidateQueries({ queryKey: ['schedules'] }) })
}
export function useDeleteSchedule() {
  const qc = useQueryClient()
  return useMutation({ mutationFn: (id: string) => apiDelete(`/v1/schedules/${id}`), onSuccess: () => qc.invalidateQueries({ queryKey: ['schedules'] }) })
}

// Runners
export function useRunners() {
  return useQuery({ queryKey: ['runners'], queryFn: () => apiFetch<T.RunnerSummary[]>('/v1/runners'), refetchInterval: 10000 })
}
export function useDeleteRunner() {
  const qc = useQueryClient()
  return useMutation({ mutationFn: (id: string) => apiDelete(`/v1/runners/${id}`), onSuccess: () => qc.invalidateQueries({ queryKey: ['runners'] }) })
}

// Executions
export function useExecutions(params?: { job_key?: string; state?: string; limit?: number }) {
  const search = new URLSearchParams()
  if (params?.job_key) search.set('job_key', params.job_key)
  if (params?.state) search.set('state', params.state)
  if (params?.limit) search.set('limit', String(params.limit))
  const qs = search.toString() ? `?${search}` : ''
  return useQuery({ queryKey: ['executions', params], queryFn: () => apiFetch<T.Execution[]>(`/v1/executions${qs}`) })
}
export function useExecutionLogs(executionId: string) {
  return useQuery({ queryKey: ['execution-logs', executionId], queryFn: () => apiFetch<T.ExecutionLogEntry[]>(`/v1/executions/${executionId}/logs`) })
}

// Dead Letters
export function useDeadLetters(jobKey?: string) {
  const params = jobKey ? `?job_key=${jobKey}` : ''
  return useQuery({ queryKey: ['dead-letters', jobKey], queryFn: () => apiFetch<T.DeadLetter[]>(`/v1/dead-letters${params}`) })
}
export function useDeadLetter(id: string) {
  return useQuery({ queryKey: ['dead-letters', id], queryFn: () => apiFetch<T.DeadLetter>(`/v1/dead-letters/${id}`) })
}
export function useDeleteDeadLetter() {
  const qc = useQueryClient()
  return useMutation({ mutationFn: (id: string) => apiDelete(`/v1/dead-letters/${id}`), onSuccess: () => qc.invalidateQueries({ queryKey: ['dead-letters'] }) })
}
