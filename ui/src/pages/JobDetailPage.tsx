import { useCallback, useState } from 'react'
import { useParams } from 'react-router'
import { useForm } from 'react-hook-form'
import * as Dialog from '@radix-ui/react-dialog'
import { Plus, Trash2, X } from 'lucide-react'
import {
  useJob,
  useSchedules,
  useExecutions,
  useCreateSchedule,
  useDeleteSchedule,
} from '@/api/hooks'
import { Badge } from '@/components/ui/badge'
import { stateVariant } from '@/components/ui/badge-variants'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Spinner } from '@/components/ui/spinner'
import { CopyButton } from '@/components/ui/copy-button'
import { RelativeTime } from '@/components/ui/relative-time'
import { useConfirm } from '@/components/ui/confirm-dialog'
import { ScheduleBuilder } from '@/components/builders/ScheduleBuilder'
import { formatDate, shortId } from '@/lib/utils'

interface ScheduleForm {
  cron_expression: string
  timezone: string
}

export function JobDetailPage() {
  const { jobKey } = useParams<{ jobKey: string }>()
  const job = useJob(jobKey!)
  const schedules = useSchedules(jobKey)
  const executions = useExecutions({ job_key: jobKey, limit: 20 })
  const createSchedule = useCreateSchedule()
  const deleteSchedule = useDeleteSchedule()
  const { confirm, dialog: confirmDialog } = useConfirm()
  const [scheduleDialogOpen, setScheduleDialogOpen] = useState(false)

  async function handleDeleteSchedule(triggerId: string, cron: string | null) {
    const ok = await confirm({
      title: 'Delete schedule?',
      description: cron
        ? `The cron schedule "${cron}" will stop firing. Existing in-flight executions are not affected.`
        : 'This schedule will stop firing. Existing in-flight executions are not affected.',
      confirmLabel: 'Delete schedule',
      destructive: true,
    })
    if (ok) deleteSchedule.mutate(triggerId)
  }

  const { register, handleSubmit, reset, formState: { errors }, setValue } =
    useForm<ScheduleForm>({ defaultValues: { cron_expression: '', timezone: '' } })
  const inputCls =
    'w-full px-3 py-2 border border-border rounded-md text-sm bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary'

  // Two ways to enter the schedule:
  //   "builder"  — form-driven, drives `cron_expression` via wasm
  //   "advanced" — raw text, same as before (cron syntax or DSL)
  // The `cron_expression` field stays a single string in the form so
  // the API call is unchanged regardless of mode.
  const [scheduleMode, setScheduleMode] = useState<'builder' | 'advanced'>('builder')
  const onBuilderChange = useCallback(
    (dsl: string) => setValue('cron_expression', dsl, { shouldValidate: true }),
    [setValue],
  )

  async function onScheduleSubmit(data: ScheduleForm) {
    if (!jobKey) return
    await createSchedule.mutateAsync({
      job_key: jobKey,
      cron_expression: data.cron_expression,
      timezone: data.timezone || undefined,
    })
    reset()
    setScheduleMode('builder')
    setScheduleDialogOpen(false)
  }

  if (job.isLoading) return <div className="flex justify-center py-12"><Spinner className="h-6 w-6" /></div>
  if (!job.data) return <p className="text-destructive text-sm">Job not found</p>

  const j = job.data
  const scheduleCount = schedules.data?.length ?? 0

  return (
    <div className="space-y-6">
      {confirmDialog}
      <div className="flex items-center gap-3">
        <span className="font-mono text-base font-semibold">{j.job_key}</span>
        <Badge variant={j.is_active ? 'ok' : 'neutral'}>{j.is_active ? 'active' : 'inactive'}</Badge>
        <CopyButton value={j.job_key} label={`Copy job key ${j.job_key}`} />
      </div>

      <Card>
        <CardContent className="pt-4">
          <dl className="grid grid-cols-2 md:grid-cols-3 gap-x-6 gap-y-3 text-sm">
            {j.description && (
              <div className="col-span-full">
                <dt className="text-xs text-muted-foreground uppercase tracking-wide mb-0.5">Description</dt>
                <dd>{j.description}</dd>
              </div>
            )}
            <div>
              <dt className="text-xs text-muted-foreground uppercase tracking-wide mb-0.5">Runner</dt>
              <dd className="font-mono text-xs">{j.assigned_runner_id || '—'}</dd>
            </div>
            <div>
              <dt className="text-xs text-muted-foreground uppercase tracking-wide mb-0.5">Updated</dt>
              <dd>{formatDate(j.updated_at)}</dd>
            </div>
          </dl>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex flex-row items-center justify-between">
          <CardTitle>Schedules ({scheduleCount})</CardTitle>
          <Dialog.Root open={scheduleDialogOpen} onOpenChange={setScheduleDialogOpen}>
            <Dialog.Trigger asChild>
              <Button size="sm"><Plus className="h-3.5 w-3.5" />Create Schedule</Button>
            </Dialog.Trigger>
            <Dialog.Portal>
              <Dialog.Overlay className="fixed inset-0 z-40 bg-black/40" />
              <Dialog.Content className="fixed left-1/2 top-1/2 z-50 w-full max-w-md -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-card p-6 shadow-xl max-h-[90vh] overflow-y-auto">
                <div className="flex items-center justify-between mb-4">
                  <Dialog.Title className="text-sm font-semibold">Create Schedule for {j.job_key}</Dialog.Title>
                  <Dialog.Close
                    aria-label="Close dialog"
                    className="text-muted-foreground hover:text-foreground"
                  >
                    <X className="h-4 w-4" />
                  </Dialog.Close>
                </div>
                {/* Mode toggle — Builder is the default for new schedules,
                    Advanced is the escape hatch for pasting cron syntax
                    or for power users who prefer the raw DSL. */}
                <div role="tablist" className="inline-flex border border-border rounded-md p-0.5 mb-3 text-xs">
                  {(['builder', 'advanced'] as const).map((m) => (
                    <button
                      key={m}
                      type="button"
                      role="tab"
                      aria-selected={scheduleMode === m}
                      onClick={() => setScheduleMode(m)}
                      className={`px-3 py-1 rounded-sm capitalize ${
                        scheduleMode === m
                          ? 'bg-primary/15 text-primary'
                          : 'text-muted-foreground hover:text-foreground'
                      }`}
                    >
                      {m === 'builder' ? 'Builder' : 'Advanced (raw)'}
                    </button>
                  ))}
                </div>
                <form onSubmit={handleSubmit(onScheduleSubmit)} className="space-y-3">
                  {scheduleMode === 'builder' ? (
                    <>
                      <ScheduleBuilder onChange={onBuilderChange} />
                      {/* Hidden RHF input — the builder writes here so
                          the form's required-validation still works. */}
                      <input
                        type="hidden"
                        {...register('cron_expression', { required: 'Required' })}
                      />
                      {errors.cron_expression && (
                        <p className="text-xs text-destructive">{errors.cron_expression.message}</p>
                      )}
                    </>
                  ) : (
                    <div>
                      <input
                        {...register('cron_expression', { required: 'Required' })}
                        placeholder="Cron or interval (e.g. */15 * * * *, 5m, every 5 minutes)"
                        className={inputCls}
                      />
                      {errors.cron_expression && (
                        <p className="text-xs text-destructive mt-1">{errors.cron_expression.message}</p>
                      )}
                    </div>
                  )}
                  <input
                    {...register('timezone')}
                    placeholder="Timezone (optional, e.g. Europe/Vienna)"
                    className={inputCls}
                  />
                  <div className="flex justify-end gap-2 pt-2">
                    <Dialog.Close asChild><Button variant="secondary" size="sm" type="button">Cancel</Button></Dialog.Close>
                    <Button type="submit" size="sm" disabled={createSchedule.isPending}>
                      {createSchedule.isPending ? <><Spinner className="h-3.5 w-3.5" />Saving…</> : 'Create'}
                    </Button>
                  </div>
                </form>
              </Dialog.Content>
            </Dialog.Portal>
          </Dialog.Root>
        </CardHeader>
        <CardContent className="p-0">
          {schedules.isLoading ? (
            <div className="flex justify-center py-6"><Spinner className="h-4 w-4" /></div>
          ) : (
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border">
                  {['Cron', 'Timezone', 'Enabled', 'Managed By', ''].map((h, i) => (
                    <th key={i} className="px-3 py-2.5 text-left text-xs font-medium text-muted-foreground uppercase tracking-wide">{h}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {scheduleCount === 0 && (
                  <tr><td colSpan={5} className="px-3 py-6 text-center text-sm text-muted-foreground">No schedules</td></tr>
                )}
                {schedules.data?.map((s) => (
                  <tr key={s.trigger_id} className="border-b border-border last:border-0 hover:bg-accent/30 transition-colors">
                    <td className="px-3 py-2.5 font-mono text-xs">{s.cron_expression || '—'}</td>
                    <td className="px-3 py-2.5 text-muted-foreground">{s.timezone || 'UTC'}</td>
                    <td className="px-3 py-2.5">
                      <Badge variant={s.enabled ? 'ok' : 'neutral'}>{s.enabled ? 'enabled' : 'disabled'}</Badge>
                    </td>
                    <td className="px-3 py-2.5 text-muted-foreground">{s.managed_by}</td>
                    <td className="px-3 py-2.5 text-right">
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => handleDeleteSchedule(s.trigger_id, s.cron_expression)}
                        aria-label={`Delete schedule`}
                        className="h-7 w-7 p-0 text-muted-foreground hover:text-destructive"
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Recent Executions</CardTitle>
        </CardHeader>
        <CardContent className="p-0">
          {executions.isLoading ? (
            <div className="flex justify-center py-6"><Spinner className="h-4 w-4" /></div>
          ) : (
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border">
                  {['ID', 'State', 'Runner', 'Fire At', 'Duration'].map((h) => (
                    <th key={h} className="px-3 py-2.5 text-left text-xs font-medium text-muted-foreground uppercase tracking-wide">{h}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {executions.data?.length === 0 && (
                  <tr><td colSpan={5} className="px-3 py-6 text-center text-sm text-muted-foreground">No executions yet</td></tr>
                )}
                {executions.data?.map((e) => (
                  <tr key={e.id} className="border-b border-border last:border-0 hover:bg-accent/30 transition-colors">
                    <td className="px-3 py-2.5 font-mono text-xs text-muted-foreground" title={e.id}>{shortId(e.id)}</td>
                    <td className="px-3 py-2.5">
                      <Badge variant={stateVariant(e.state)}>{e.state}</Badge>
                    </td>
                    <td className="px-3 py-2.5 text-muted-foreground font-mono text-xs">
                      <div className="max-w-[14rem] truncate" title={e.runner_id || undefined}>{e.runner_id || '—'}</div>
                    </td>
                    <td className="px-3 py-2.5 text-muted-foreground">
                      <RelativeTime iso={e.fire_at} />
                    </td>
                    <td className="px-3 py-2.5 text-muted-foreground">{e.duration_ms ? `${e.duration_ms}ms` : '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
