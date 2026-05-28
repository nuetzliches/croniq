import { useEffect } from 'react'
import { useForm } from 'react-hook-form'
import * as Dialog from '@radix-ui/react-dialog'
import { X, AlertCircle } from 'lucide-react'
import { useCreateSchedule, useUpdateSchedule } from '@/api/hooks'
import type { TriggerDefinition } from '@/api/types'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'

interface Props {
  jobKey: string
  schedule: TriggerDefinition | null
  open: boolean
  onOpenChange: (open: boolean) => void
}

interface ScheduleForm {
  cron_expression: string
  timezone: string
  calendar: string
  window: string
  enabled: boolean
}

const inputCls =
  'w-full px-3 py-2 border border-border rounded-md text-sm bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary'

export function ScheduleDialog({ jobKey, schedule, open, onOpenChange }: Props) {
  const createSchedule = useCreateSchedule()
  const updateSchedule = useUpdateSchedule()
  const isEdit = schedule !== null
  const {
    register,
    handleSubmit,
    reset,
    formState: { errors },
  } = useForm<ScheduleForm>()

  useEffect(() => {
    if (open) {
      reset({
        cron_expression: schedule?.cron_expression ?? '',
        timezone: schedule?.timezone ?? '',
        calendar: schedule?.calendar ?? '',
        window: schedule?.window ?? '',
        enabled: schedule?.enabled ?? true,
      })
    }
  }, [open, schedule, reset])

  const pending = createSchedule.isPending || updateSchedule.isPending
  const error = createSchedule.error ?? updateSchedule.error

  async function onSubmit(data: ScheduleForm) {
    const cron = data.cron_expression.trim()
    const tz = data.timezone.trim()
    const cal = data.calendar.trim()
    const win = data.window.trim()

    if (isEdit && schedule) {
      await updateSchedule.mutateAsync({
        trigger_id: schedule.trigger_id,
        cron_expression: cron,
        timezone: tz === '' ? null : tz,
        calendar: cal === '' ? null : cal,
        window: win === '' ? null : win,
        enabled: data.enabled,
      })
    } else {
      await createSchedule.mutateAsync({
        job_key: jobKey,
        cron_expression: cron,
        timezone: tz === '' ? undefined : tz,
        calendar: cal === '' ? undefined : cal,
        window: win === '' ? undefined : win,
        enabled: data.enabled,
      })
    }
    onOpenChange(false)
  }

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/40" />
        <Dialog.Content className="fixed left-1/2 top-1/2 z-50 w-full max-w-md -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-card p-6 shadow-xl">
          <div className="flex items-center justify-between mb-4">
            <Dialog.Title className="text-sm font-semibold">
              {isEdit ? 'Edit Schedule' : 'New Schedule'} — {jobKey}
            </Dialog.Title>
            <Dialog.Close
              aria-label="Close dialog"
              className="text-muted-foreground hover:text-foreground"
            >
              <X className="h-4 w-4" />
            </Dialog.Close>
          </div>
          <form onSubmit={handleSubmit(onSubmit)} className="space-y-3">
            <div>
              <label className="block text-xs text-muted-foreground mb-1">
                Cron expression
              </label>
              <input
                {...register('cron_expression', { required: 'Required' })}
                autoFocus
                placeholder="*/5 * * * *  or  every 5 minutes"
                className={`${inputCls} font-mono`}
              />
              {errors.cron_expression ? (
                <p className="text-xs text-destructive mt-1">{errors.cron_expression.message}</p>
              ) : (
                <p className="text-[11px] text-muted-foreground mt-1">
                  Standard 5-field cron or Croniq DSL shorthand (e.g. <code>every 5 minutes</code>).
                </p>
              )}
            </div>
            <div>
              <label className="block text-xs text-muted-foreground mb-1">
                Timezone
              </label>
              <input
                {...register('timezone')}
                placeholder="Europe/Berlin"
                className={inputCls}
              />
              <p className="text-[11px] text-muted-foreground mt-1">
                IANA timezone. Empty means UTC.
              </p>
            </div>
            <div>
              <label className="block text-xs text-muted-foreground mb-1">
                Calendar
              </label>
              <input
                {...register('calendar')}
                placeholder="eu-business-hours"
                className={inputCls}
              />
              <p className="text-[11px] text-muted-foreground mt-1">
                Calendar name to gate firing. Empty for no calendar.
              </p>
            </div>
            <div>
              <label className="block text-xs text-muted-foreground mb-1">
                Window
              </label>
              <input
                {...register('window')}
                placeholder="08:00-18:00"
                className={inputCls}
              />
              <p className="text-[11px] text-muted-foreground mt-1">
                Optional inline time window. Empty for always-on.
              </p>
            </div>
            <label className="flex items-center gap-2 text-xs">
              <input type="checkbox" {...register('enabled')} className="h-3.5 w-3.5" />
              Enabled
            </label>
            {error && (
              <p className="text-xs text-destructive flex items-center gap-1">
                <AlertCircle className="h-3.5 w-3.5" />
                {String(error)}
              </p>
            )}
            <div className="flex justify-end gap-2 pt-2">
              <Dialog.Close asChild>
                <Button variant="secondary" size="sm" type="button">Cancel</Button>
              </Dialog.Close>
              <Button type="submit" size="sm" disabled={pending}>
                {pending ? <><Spinner className="h-3.5 w-3.5" />Saving…</> : isEdit ? 'Save changes' : 'Create schedule'}
              </Button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
