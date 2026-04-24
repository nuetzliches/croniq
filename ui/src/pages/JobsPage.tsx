import { useState } from 'react'
import { Link } from 'react-router'
import { useForm } from 'react-hook-form'
import * as Dialog from '@radix-ui/react-dialog'
import * as Switch from '@radix-ui/react-switch'
import * as Tooltip from '@radix-ui/react-tooltip'
import { Plus, Play, Trash2, X, AlertCircle } from 'lucide-react'
import {
  useJobs, useRegisterJob, useDeleteJob, useActivateJob, useDeactivateJob,
  useTriggerJob, useExecutions,
} from '@/api/hooks'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent } from '@/components/ui/card'
import { EmptyState } from '@/components/ui/empty-state'
import { Spinner } from '@/components/ui/spinner'
import type { Execution } from '@/api/types'

interface RegisterForm {
  job_key: string
  description: string
  schedule: string
  timezone: string
  timeout: string
}

function HealthPill({ executions }: { executions: Execution[] }) {
  const last20 = executions.slice(0, 20)
  if (last20.length === 0) return <span className="text-xs text-muted-foreground">no runs</span>
  const ok = last20.filter(e => e.state === 'completed').length
  return (
    <Tooltip.Root>
      <Tooltip.Trigger asChild>
        <div className="flex gap-0.5 items-center cursor-default" aria-label={`${ok}/${last20.length} successful`}>
          {last20.map((e, i) => (
            <span key={i} className={`inline-block w-1.5 h-3.5 rounded-sm ${
              e.state === 'completed' ? 'bg-status-ok-fg' :
              e.state === 'failed' || e.state === 'dead' ? 'bg-status-err-fg' :
              'bg-status-neutral-fg opacity-40'
            }`} />
          ))}
        </div>
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content className="z-50 rounded-md bg-foreground px-2.5 py-1 text-xs text-background shadow-md">
          {ok}/{last20.length} successful
          <Tooltip.Arrow className="fill-foreground" />
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  )
}

const inputCls = 'w-full px-3 py-2 border border-border rounded-md text-sm bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary'

export function JobsPage() {
  const jobs = useJobs()
  const registerJob = useRegisterJob()
  const deleteJob = useDeleteJob()
  const activateJob = useActivateJob()
  const deactivateJob = useDeactivateJob()
  const triggerJob = useTriggerJob()
  const allExecs = useExecutions({ limit: 200 })
  const [open, setOpen] = useState(false)
  const [triggeredId, setTriggeredId] = useState<string | null>(null)
  const [triggerError, setTriggerError] = useState<string | null>(null)
  const [toggleError, setToggleError] = useState<string | null>(null)

  const { register, handleSubmit, reset, formState: { errors } } = useForm<RegisterForm>({
    defaultValues: { timeout: '5m' }
  })

  const execsByJob = (allExecs.data ?? []).reduce<Record<string, Execution[]>>((acc, e) => {
    ;(acc[e.job_key] ??= []).push(e)
    return acc
  }, {})

  async function onSubmit(data: RegisterForm) {
    await registerJob.mutateAsync({
      job_key: data.job_key,
      schedule: data.schedule,
      timezone: data.timezone || undefined,
      timeout: data.timeout || undefined,
      description: data.description || undefined,
    })
    reset()
    setOpen(false)
  }

  async function handleTrigger(jobKey: string) {
    setTriggeredId(jobKey)
    setTriggerError(null)
    try {
      await triggerJob.mutateAsync(jobKey)
    } catch (err) {
      setTriggerError(err instanceof Error ? err.message : 'Trigger failed')
    } finally {
      setTriggeredId(null)
    }
  }

  async function handleToggle(jobKey: string, isActive: boolean) {
    setToggleError(null)
    const mutation = isActive ? deactivateJob : activateJob
    const verb = isActive ? 'deactivate' : 'activate'
    try {
      await mutation.mutateAsync(jobKey)
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'unknown error'
      // 409 from the backend typically means the job is DSL-managed
      // (its lifecycle is owned by the Croniqfile). Surface that plainly
      // so users don't think the click was swallowed.
      const body = /409|conflict/i.test(msg)
        ? 'DSL-managed jobs cannot be toggled via the UI; edit the Croniqfile instead.'
        : msg
      setToggleError(`Could not ${verb} ${jobKey}: ${body}`)
    }
  }

  return (
    <Tooltip.Provider delayDuration={200}>
      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <p className="text-sm text-muted-foreground">{jobs.data?.length ?? 0} jobs registered</p>
          <Dialog.Root open={open} onOpenChange={setOpen}>
            <Dialog.Trigger asChild>
              <Button size="sm"><Plus className="h-3.5 w-3.5" />Create Job</Button>
            </Dialog.Trigger>
            <Dialog.Portal>
              <Dialog.Overlay className="fixed inset-0 z-40 bg-black/40" />
              <Dialog.Content className="fixed left-1/2 top-1/2 z-50 w-full max-w-md -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-card p-6 shadow-xl">
                <div className="flex items-center justify-between mb-4">
                  <Dialog.Title className="text-sm font-semibold">Create Job</Dialog.Title>
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
                    <input {...register('schedule', { required: 'Required' })} placeholder="Schedule (e.g. 5m, 1h, */15 * * * *)" className={inputCls} />
                    {errors.schedule && <p className="text-xs text-destructive mt-1">{errors.schedule.message}</p>}
                  </div>
                  <div className="grid grid-cols-2 gap-3">
                    <input {...register('timeout')} placeholder="Timeout (default: 5m)" className={inputCls} />
                    <input {...register('timezone')} placeholder="Timezone (e.g. Europe/Vienna)" className={inputCls} />
                  </div>
                  <input {...register('description')} placeholder="Description (optional)" className={inputCls} />
                  {registerJob.error && (
                    <p className="text-xs text-destructive flex items-center gap-1">
                      <AlertCircle className="h-3.5 w-3.5" />{String(registerJob.error)}
                    </p>
                  )}
                  <div className="flex justify-end gap-2 pt-2">
                    <Dialog.Close asChild><Button variant="secondary" size="sm" type="button">Cancel</Button></Dialog.Close>
                    <Button type="submit" size="sm" disabled={registerJob.isPending}>
                      {registerJob.isPending ? <><Spinner className="h-3.5 w-3.5" />Creating…</> : 'Create & Schedule'}
                    </Button>
                  </div>
                </form>
              </Dialog.Content>
            </Dialog.Portal>
          </Dialog.Root>
        </div>

        {triggerError && (
          <div className="flex items-center gap-2 text-xs text-destructive bg-destructive/10 border border-destructive/20 rounded-md px-3 py-2">
            <AlertCircle className="h-3.5 w-3.5 shrink-0" />
            {triggerError}
          </div>
        )}

        {toggleError && (
          <div className="flex items-center gap-2 text-xs text-destructive bg-destructive/10 border border-destructive/20 rounded-md px-3 py-2">
            <AlertCircle className="h-3.5 w-3.5 shrink-0" />
            {toggleError}
          </div>
        )}

        {jobs.isLoading && <div className="flex justify-center py-12"><Spinner className="h-6 w-6" /></div>}

        {!jobs.isLoading && jobs.data?.length === 0 && (
          <EmptyState
            icon={<Plus className="h-10 w-10" />}
            title="No jobs yet"
            description="Create a job or register via the Runner SDK"
            action={<Button size="sm" onClick={() => setOpen(true)}><Plus className="h-3.5 w-3.5" />Create Job</Button>}
          />
        )}

        <div className="space-y-2">
          {jobs.data?.map((j) => (
            <Card key={j.job_key}>
              <CardContent className="py-3">
                <div className="flex items-center gap-4">
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <Link to={`/jobs/${j.job_key}`} className="font-mono text-sm text-primary hover:underline truncate">
                        {j.job_key}
                      </Link>
                      <Badge variant={j.is_active ? 'ok' : 'neutral'}>
                        {j.is_active ? 'active' : 'inactive'}
                      </Badge>
                    </div>
                    {j.description && <p className="text-xs text-muted-foreground mt-0.5 truncate">{j.description}</p>}
                  </div>

                  <div className="shrink-0">
                    <HealthPill executions={execsByJob[j.job_key] ?? []} />
                  </div>

                  {/* Activate/Deactivate toggle — Tooltip.Trigger wraps a
                      span, not the Switch itself. Radix Slot's asChild merge
                      collides with Switch.Root on `data-state` (Tooltip's
                      open/closed overwrites Switch's checked/unchecked),
                      which blanks out the track color AND swallows the
                      click handler. The span receives the Tooltip data
                      attributes cleanly and the Switch inside behaves as
                      intended. */}
                  <Tooltip.Root>
                    <Tooltip.Trigger asChild>
                      <span className="inline-flex">
                        <Switch.Root
                          checked={j.is_active}
                          onCheckedChange={() => handleToggle(j.job_key, j.is_active)}
                          disabled={activateJob.isPending || deactivateJob.isPending}
                          className="relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary data-[state=checked]:bg-primary data-[state=unchecked]:bg-border disabled:opacity-50 disabled:cursor-not-allowed"
                          aria-label={`${j.is_active ? 'Deactivate' : 'Activate'} ${j.job_key}`}
                        >
                          <Switch.Thumb className="pointer-events-none block h-4 w-4 rounded-full bg-white shadow-lg ring-0 transition-transform data-[state=checked]:translate-x-4 data-[state=unchecked]:translate-x-0" />
                        </Switch.Root>
                      </span>
                    </Tooltip.Trigger>
                    <Tooltip.Portal>
                      <Tooltip.Content className="z-50 rounded-md bg-foreground px-2 py-1 text-xs text-background shadow-md">
                        {j.is_active ? 'Deactivate' : 'Activate'}
                        <Tooltip.Arrow className="fill-foreground" />
                      </Tooltip.Content>
                    </Tooltip.Portal>
                  </Tooltip.Root>

                  {/* Trigger */}
                  <Tooltip.Root>
                    <Tooltip.Trigger asChild>
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => handleTrigger(j.job_key)}
                        disabled={triggeredId === j.job_key}
                        aria-label={`Trigger ${j.job_key}`}
                        className="h-7 w-7 p-0"
                      >
                        {triggeredId === j.job_key ? <Spinner className="h-3.5 w-3.5" /> : <Play className="h-3.5 w-3.5" />}
                      </Button>
                    </Tooltip.Trigger>
                    <Tooltip.Portal>
                      <Tooltip.Content className="z-50 rounded-md bg-foreground px-2 py-1 text-xs text-background shadow-md">
                        Trigger now
                        <Tooltip.Arrow className="fill-foreground" />
                      </Tooltip.Content>
                    </Tooltip.Portal>
                  </Tooltip.Root>

                  {/* Delete */}
                  <Tooltip.Root>
                    <Tooltip.Trigger asChild>
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => deleteJob.mutate(j.job_key)}
                        aria-label={`Delete ${j.job_key}`}
                        className="h-7 w-7 p-0 text-muted-foreground hover:text-destructive"
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </Button>
                    </Tooltip.Trigger>
                    <Tooltip.Portal>
                      <Tooltip.Content className="z-50 rounded-md bg-foreground px-2 py-1 text-xs text-background shadow-md">
                        Delete job
                        <Tooltip.Arrow className="fill-foreground" />
                      </Tooltip.Content>
                    </Tooltip.Portal>
                  </Tooltip.Root>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      </div>
    </Tooltip.Provider>
  )
}
