import { useState } from 'react'
import { useForm } from 'react-hook-form'
import * as Dialog from '@radix-ui/react-dialog'
import { Plus, Trash2, X } from 'lucide-react'
import { useSchedules, useCreateSchedule, useDeleteSchedule } from '@/api/hooks'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { EmptyState } from '@/components/ui/empty-state'
import { Spinner } from '@/components/ui/spinner'

interface ScheduleForm {
  job_key: string
  cron_expression: string
  timezone: string
}

export function SchedulesPage() {
  const { data: schedules, isLoading } = useSchedules()
  const createSchedule = useCreateSchedule()
  const deleteSchedule = useDeleteSchedule()
  const [open, setOpen] = useState(false)

  const { register, handleSubmit, reset, formState: { errors } } = useForm<ScheduleForm>()

  const inputCls = 'w-full px-3 py-2 border border-border rounded-md text-sm bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary'

  async function onSubmit(data: ScheduleForm) {
    await createSchedule.mutateAsync({
      job_key: data.job_key,
      cron_expression: data.cron_expression,
      timezone: data.timezone || undefined,
    })
    reset()
    setOpen(false)
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">{schedules?.length ?? 0} schedules</p>
        <Dialog.Root open={open} onOpenChange={setOpen}>
          <Dialog.Trigger asChild>
            <Button size="sm"><Plus className="h-3.5 w-3.5" />Create Schedule</Button>
          </Dialog.Trigger>
          <Dialog.Portal>
            <Dialog.Overlay className="fixed inset-0 z-40 bg-black/40" />
            <Dialog.Content className="fixed left-1/2 top-1/2 z-50 w-full max-w-md -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-card p-6 shadow-xl">
              <div className="flex items-center justify-between mb-4">
                <Dialog.Title className="text-sm font-semibold">Create Schedule</Dialog.Title>
                <Dialog.Close className="text-muted-foreground hover:text-foreground">
                  <X className="h-4 w-4" />
                </Dialog.Close>
              </div>
              <form onSubmit={handleSubmit(onSubmit)} className="space-y-3">
                <div>
                  <input {...register('job_key', { required: 'Required' })} placeholder="Job key (e.g. billing:invoice)" className={inputCls} />
                  {errors.job_key && <p className="text-xs text-destructive mt-1">{errors.job_key.message}</p>}
                </div>
                <div>
                  <input {...register('cron_expression', { required: 'Required' })} placeholder="Cron or interval (e.g. */15 * * * *, 5m)" className={inputCls} />
                  {errors.cron_expression && <p className="text-xs text-destructive mt-1">{errors.cron_expression.message}</p>}
                </div>
                <input {...register('timezone')} placeholder="Timezone (optional, e.g. Europe/Vienna)" className={inputCls} />
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
      </div>

      {isLoading && <div className="flex justify-center py-12"><Spinner className="h-6 w-6" /></div>}

      {!isLoading && schedules?.length === 0 && (
        <EmptyState
          icon={<Plus className="h-10 w-10" />}
          title="No schedules"
          description="Create a schedule to start triggering jobs automatically"
          action={<Button size="sm" onClick={() => setOpen(true)}><Plus className="h-3.5 w-3.5" />Create Schedule</Button>}
        />
      )}

      {(schedules?.length ?? 0) > 0 && (
        <div className="rounded-lg border border-border bg-card overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border">
                {['Job Key', 'Cron', 'Timezone', 'Enabled', 'Managed By', ''].map((h, i) => (
                  <th key={i} className="px-3 py-2.5 text-left text-xs font-medium text-muted-foreground uppercase tracking-wide">{h}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {schedules?.map((s) => (
                <tr key={s.trigger_id} className="border-b border-border last:border-0 hover:bg-accent/30 transition-colors">
                  <td className="px-3 py-2.5 font-mono text-xs">{s.job_key}</td>
                  <td className="px-3 py-2.5 font-mono text-xs text-muted-foreground">{s.cron_expression || '—'}</td>
                  <td className="px-3 py-2.5 text-muted-foreground">{s.timezone || 'UTC'}</td>
                  <td className="px-3 py-2.5">
                    <Badge variant={s.enabled ? 'ok' : 'neutral'}>{s.enabled ? 'enabled' : 'disabled'}</Badge>
                  </td>
                  <td className="px-3 py-2.5 text-muted-foreground">{s.managed_by}</td>
                  <td className="px-3 py-2.5 text-right">
                    <Button
                      variant="ghost" size="sm"
                      onClick={() => deleteSchedule.mutate(s.trigger_id)}
                      aria-label={`Delete schedule for ${s.job_key}`}
                      className="h-7 w-7 p-0 text-muted-foreground hover:text-destructive"
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}
