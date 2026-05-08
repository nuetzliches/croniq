import { useState, useCallback } from 'react'
import { Link } from 'react-router'
import { useForm } from 'react-hook-form'
import * as Dialog from '@radix-ui/react-dialog'
import * as Switch from '@radix-ui/react-switch'
import * as Tooltip from '@radix-ui/react-tooltip'
import { Plus, Play, Trash2, X, AlertCircle, Pencil, Download } from 'lucide-react'
import {
  useJobs, useRegisterJob, useDeleteJob, useActivateJob, useDeactivateJob,
  useTriggerJob, useExecutions, useSchedules, useAdoptJob, useJobTags,
} from '@/api/hooks'
import { ScheduleBuilder } from '@/components/builders/ScheduleBuilder'
import { TimezoneInput } from '@/components/ui/timezone-input'
import { CalendarPicker } from '@/components/ui/calendar-picker'
import { EditJobDialog } from '@/components/EditJobDialog'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent } from '@/components/ui/card'
import { EmptyState } from '@/components/ui/empty-state'
import { Spinner } from '@/components/ui/spinner'
import { useConfirm } from '@/components/ui/confirm-dialog'
import type { Execution, JobDefinition } from '@/api/types'

interface RegisterForm {
  job_key: string
  description: string
  schedule: string
  timezone: string
  timeout: string
  calendar: string
}

function HealthPill({ executions }: { executions: Execution[] }) {
  const last20 = executions.slice(0, 20)
  if (last20.length === 0) return <span className="text-xs text-muted-foreground">no runs</span>
  const ok = last20.filter(e => e.state === 'completed').length
  // Show the count inline so users don't need to hover. The bar chart
  // remains because at a glance it surfaces the *pattern* (recent vs
  // historical failures) better than a raw number can.
  return (
    <Tooltip.Root>
      <Tooltip.Trigger asChild>
        <div className="flex gap-1.5 items-center cursor-default" aria-label={`${ok}/${last20.length} successful`}>
          <div className="flex gap-0.5 items-center">
            {last20.map((e, i) => (
              <span key={i} className={`inline-block w-1.5 h-3.5 rounded-sm ${
                e.state === 'completed' ? 'bg-status-ok-fg' :
                e.state === 'failed' || e.state === 'dead' ? 'bg-status-err-fg' :
                'bg-status-neutral-fg opacity-40'
              }`} />
            ))}
          </div>
          <span className="text-xs text-muted-foreground tabular-nums">{ok}/{last20.length}</span>
        </div>
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content className="z-50 rounded-md bg-foreground px-2.5 py-1 text-xs text-background shadow-md">
          Last {last20.length} runs · {ok} successful
          <Tooltip.Arrow className="fill-foreground" />
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  )
}

const inputCls = 'w-full px-3 py-2 border border-border rounded-md text-sm bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary'

export function JobsPage() {
  const jobs = useJobs()
  const tagCounts = useJobTags()
  const registerJob = useRegisterJob()
  const deleteJob = useDeleteJob()
  const activateJob = useActivateJob()
  const deactivateJob = useDeactivateJob()
  const triggerJob = useTriggerJob()
  const adoptJob = useAdoptJob()
  const allExecs = useExecutions({ limit: 200 })
  const allSchedules = useSchedules()
  const { confirm, dialog: confirmDialog } = useConfirm()
  const [open, setOpen] = useState(false)
  const [triggeredId, setTriggeredId] = useState<string | null>(null)
  const [triggerError, setTriggerError] = useState<string | null>(null)
  const [toggleError, setToggleError] = useState<string | null>(null)
  const [adoptError, setAdoptError] = useState<string | null>(null)
  const [editingJob, setEditingJob] = useState<JobDefinition | null>(null)
  const [activeTags, setActiveTags] = useState<Set<string>>(new Set())

  const toggleTag = (tag: string) =>
    setActiveTags((prev) => {
      const next = new Set(prev)
      if (next.has(tag)) next.delete(tag)
      else next.add(tag)
      return next
    })

  // AND-semantics across selected tags: a job must carry all of them.
  const filteredJobs = (jobs.data ?? []).filter((j) => {
    if (activeTags.size === 0) return true
    const have = new Set(j.tags ?? [])
    for (const t of activeTags) if (!have.has(t)) return false
    return true
  })

  const { register, handleSubmit, reset, setValue, formState: { errors } } = useForm<RegisterForm>({
    defaultValues: { timeout: '5m' },
  })
  const [scheduleMode, setScheduleMode] = useState<'builder' | 'advanced'>('builder')
  const onBuilderChange = useCallback(
    (dsl: string) => setValue('schedule', dsl, { shouldValidate: true }),
    [setValue],
  )

  const execsByJob = (allExecs.data ?? []).reduce<Record<string, Execution[]>>((acc, e) => {
    ;(acc[e.job_key] ??= []).push(e)
    return acc
  }, {})

  // A job is DSL-managed when any of its schedules came from the Croniqfile.
  // The backend refuses UI toggle/delete for those (HTTP 409), so disable
  // the controls and tell the user why instead of letting them click into
  // an error.
  const dslManagedJobs = new Set(
    (allSchedules.data ?? [])
      .filter((s) => s.managed_by === 'dsl')
      .map((s) => s.job_key)
  )

  async function onSubmit(data: RegisterForm) {
    await registerJob.mutateAsync({
      job_key: data.job_key,
      schedule: data.schedule,
      timezone: data.timezone || undefined,
      timeout: data.timeout || undefined,
      description: data.description || undefined,
      calendar: data.calendar || undefined,
    })
    reset()
    setScheduleMode('builder')
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

  async function handleDelete(jobKey: string) {
    const ok = await confirm({
      title: `Delete job ${jobKey}?`,
      description: 'The job, its schedules, and any associated trigger state are removed permanently. Past executions and dead letters are preserved.',
      confirmLabel: 'Delete job',
      destructive: true,
    })
    if (ok) deleteJob.mutate(jobKey)
  }

  async function handleAdopt(jobKey: string) {
    const ok = await confirm({
      title: `Adopt job ${jobKey}?`,
      description:
        'The Croniqfile job + its trigger are copied into the API store. The DSL definition is ignored on the next reload until you unadopt. Requires `policy { dsl_adopt_on_mutate true }`.',
      confirmLabel: 'Adopt to edit',
    })
    if (!ok) return
    setAdoptError(null)
    try {
      await adoptJob.mutateAsync(jobKey)
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      const m = msg.match(/^409:\s*(.+)$/s)
      if (m) {
        try {
          const parsed = JSON.parse(m[1])
          setAdoptError(parsed.message ?? msg)
        } catch {
          setAdoptError(m[1])
        }
      } else {
        setAdoptError(msg)
      }
    }
  }

  return (
    <Tooltip.Provider delayDuration={200}>
      {confirmDialog}
      <EditJobDialog
        job={editingJob}
        open={editingJob !== null}
        onOpenChange={(o) => { if (!o) setEditingJob(null) }}
      />
      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <p className="text-sm text-muted-foreground">{jobs.data?.length ?? 0} jobs registered</p>
          <Dialog.Root open={open} onOpenChange={setOpen}>
            <Dialog.Trigger asChild>
              <Button size="sm"><Plus className="h-3.5 w-3.5" />Create Job</Button>
            </Dialog.Trigger>
            <Dialog.Portal>
              <Dialog.Overlay className="fixed inset-0 z-40 bg-black/40" />
              <Dialog.Content className="fixed left-1/2 top-1/2 z-50 w-full max-w-lg -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-card p-6 shadow-xl">
                <div className="flex items-center justify-between mb-4">
                  <Dialog.Title className="text-sm font-semibold">Create Job</Dialog.Title>
                  <Dialog.Close
                    aria-label="Close dialog"
                    className="text-muted-foreground hover:text-foreground"
                  >
                    <X className="h-4 w-4" />
                  </Dialog.Close>
                </div>
                <form onSubmit={handleSubmit(onSubmit)} className="space-y-5 max-h-[80vh] overflow-y-auto">
                  {/* Section: Job — identity + execution metadata. The
                      schedule below is technically a separate aggregate
                      (a job can carry many schedules), but creating a
                      job without one is rarely what users want, so we
                      prompt for both here and split visually instead. */}
                  <fieldset className="space-y-3">
                    <legend className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                      Job
                    </legend>
                    <div>
                      <input {...register('job_key', { required: 'Required' })} placeholder="Job key (e.g. billing:invoice)" className={inputCls} />
                      {errors.job_key && <p className="text-xs text-destructive mt-1">{errors.job_key.message}</p>}
                    </div>
                    <input {...register('description')} placeholder="Description (optional)" className={inputCls} />
                    <div>
                      <label className="block text-xs text-muted-foreground mb-1">
                        Execution timeout
                      </label>
                      <input
                        {...register('timeout')}
                        placeholder="e.g. 5m, 30s, 1h"
                        className={inputCls}
                      />
                      <p className="text-[11px] text-muted-foreground mt-1">
                        How long an execution may run before being killed. Default: 5m.
                      </p>
                    </div>
                  </fieldset>

                  <hr className="border-border" />

                  {/* Section: Schedule — when this job fires + calendar
                      gating. Mirrors the standalone Create Schedule
                      dialog on JobDetailPage so users see the same form
                      shape in both places. */}
                  <fieldset className="space-y-3">
                    <legend className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                      Schedule
                    </legend>
                    <div>
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
                            {m}
                          </button>
                        ))}
                      </div>
                      {scheduleMode === 'builder' ? (
                        <>
                          <ScheduleBuilder onChange={onBuilderChange} />
                          <input type="hidden" {...register('schedule', { required: 'Required' })} />
                        </>
                      ) : (
                        <input {...register('schedule', { required: 'Required' })} placeholder="Schedule (e.g. 5m, 1h, */15 * * * *)" className={inputCls} />
                      )}
                      {errors.schedule && <p className="text-xs text-destructive mt-1">{errors.schedule.message}</p>}
                    </div>
                    <TimezoneInput
                      {...register('timezone')}
                      className={inputCls}
                      showDetectedHint
                    />
                    <CalendarPicker {...register('calendar')} />
                  </fieldset>

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

        {adoptError && (
          <div className="flex items-center gap-2 text-xs text-destructive bg-destructive/10 border border-destructive/20 rounded-md px-3 py-2">
            <AlertCircle className="h-3.5 w-3.5 shrink-0" />
            {adoptError}
          </div>
        )}

        {(tagCounts.data?.length ?? 0) > 0 && (
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="text-xs text-muted-foreground mr-1">Tags:</span>
            {tagCounts.data?.map((tc) => {
              const active = activeTags.has(tc.tag)
              return (
                <button
                  key={tc.tag}
                  type="button"
                  onClick={() => toggleTag(tc.tag)}
                  className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs transition-colors ${
                    active
                      ? 'bg-primary text-primary-foreground'
                      : 'bg-accent text-accent-foreground hover:bg-accent/70'
                  }`}
                  aria-pressed={active}
                >
                  <span className="font-mono">{tc.tag}</span>
                  <span className="opacity-70 tabular-nums">{tc.count}</span>
                </button>
              )
            })}
            {activeTags.size > 0 && (
              <button
                type="button"
                onClick={() => setActiveTags(new Set())}
                className="text-xs text-muted-foreground hover:text-foreground underline ml-1"
              >
                clear
              </button>
            )}
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

        {!jobs.isLoading && (jobs.data?.length ?? 0) > 0 && filteredJobs.length === 0 && (
          <p className="text-sm text-muted-foreground py-6 text-center">
            No jobs match the selected tags.
          </p>
        )}

        <div className="space-y-2">
          {filteredJobs.map((j) => {
            const isDslManaged = dslManagedJobs.has(j.job_key)
            const toggleTip = isDslManaged
              ? 'Managed by Croniqfile — edit the DSL to change this'
              : (j.is_active ? 'Deactivate' : 'Activate')
            const deleteTip = isDslManaged
              ? 'Managed by Croniqfile — delete via the DSL'
              : 'Delete job'
            return (
            <Card key={j.job_key}>
              <CardContent className="py-3">
                <div className="flex items-center gap-4">
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 flex-wrap">
                      <Link to={`/jobs/${j.job_key}`} className="font-mono text-sm text-primary hover:underline truncate">
                        {j.job_key}
                      </Link>
                      <Badge variant={j.is_active ? 'ok' : 'neutral'}>
                        {j.is_active ? 'active' : 'inactive'}
                      </Badge>
                      {isDslManaged && (
                        <Badge variant="neutral" className="font-mono">dsl</Badge>
                      )}
                      {(j.tags ?? []).map((t) => (
                        <button
                          key={t}
                          type="button"
                          onClick={(e) => { e.preventDefault(); e.stopPropagation(); toggleTag(t) }}
                          className="inline-flex items-center rounded-full bg-accent px-2 py-0.5 text-[10px] font-mono text-accent-foreground hover:bg-accent/70"
                          title={`Filter by ${t}`}
                        >
                          {t}
                        </button>
                      ))}
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
                          disabled={isDslManaged || activateJob.isPending || deactivateJob.isPending}
                          className="relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary data-[state=checked]:bg-primary data-[state=unchecked]:bg-border disabled:opacity-50 disabled:cursor-not-allowed"
                          aria-label={`${j.is_active ? 'Deactivate' : 'Activate'} ${j.job_key}`}
                        >
                          <Switch.Thumb className="pointer-events-none block h-4 w-4 rounded-full bg-white shadow-lg ring-0 transition-transform data-[state=checked]:translate-x-4 data-[state=unchecked]:translate-x-0" />
                        </Switch.Root>
                      </span>
                    </Tooltip.Trigger>
                    <Tooltip.Portal>
                      <Tooltip.Content className="z-50 rounded-md bg-foreground px-2 py-1 text-xs text-background shadow-md">
                        {toggleTip}
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

                  {/* Adopt — only shown for DSL-managed jobs. Copies the
                      DSL definition into the API store so the user can
                      edit it without touching the Croniqfile. Requires
                      `policy { dsl_adopt_on_mutate true }` server-side. */}
                  {isDslManaged && (
                    <Tooltip.Root>
                      <Tooltip.Trigger asChild>
                        <span className="inline-flex">
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => handleAdopt(j.job_key)}
                            disabled={adoptJob.isPending}
                            aria-label={`Adopt ${j.job_key}`}
                            className="h-7 w-7 p-0 text-muted-foreground hover:text-primary"
                          >
                            <Download className="h-3.5 w-3.5" />
                          </Button>
                        </span>
                      </Tooltip.Trigger>
                      <Tooltip.Portal>
                        <Tooltip.Content className="z-50 max-w-xs rounded-md bg-foreground px-2 py-1 text-xs text-background shadow-md">
                          Adopt to API store (requires <code>policy {`{ dsl_adopt_on_mutate true }`}</code>)
                          <Tooltip.Arrow className="fill-foreground" />
                        </Tooltip.Content>
                      </Tooltip.Portal>
                    </Tooltip.Root>
                  )}

                  {/* Edit — disabled for DSL-managed jobs (Croniqfile owns
                      them; PUT would 409). */}
                  <Tooltip.Root>
                    <Tooltip.Trigger asChild>
                      <span className="inline-flex">
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => setEditingJob(j)}
                          disabled={isDslManaged}
                          aria-label={`Edit ${j.job_key}`}
                          className="h-7 w-7 p-0 text-muted-foreground hover:text-primary disabled:opacity-50 disabled:cursor-not-allowed"
                        >
                          <Pencil className="h-3.5 w-3.5" />
                        </Button>
                      </span>
                    </Tooltip.Trigger>
                    <Tooltip.Portal>
                      <Tooltip.Content className="z-50 rounded-md bg-foreground px-2 py-1 text-xs text-background shadow-md">
                        {isDslManaged ? 'Managed by Croniqfile — adopt or edit the DSL to change' : 'Edit job'}
                        <Tooltip.Arrow className="fill-foreground" />
                      </Tooltip.Content>
                    </Tooltip.Portal>
                  </Tooltip.Root>

                  {/* Delete — disabled for DSL-managed jobs. The button sits
                      inside a span so the Tooltip still fires on hover even
                      when the button itself is disabled (disabled buttons
                      don't emit pointer events in all browsers). */}
                  <Tooltip.Root>
                    <Tooltip.Trigger asChild>
                      <span className="inline-flex">
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => handleDelete(j.job_key)}
                          disabled={isDslManaged}
                          aria-label={`Delete ${j.job_key}`}
                          className="h-7 w-7 p-0 text-muted-foreground hover:text-destructive disabled:opacity-50 disabled:cursor-not-allowed"
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </Button>
                      </span>
                    </Tooltip.Trigger>
                    <Tooltip.Portal>
                      <Tooltip.Content className="z-50 rounded-md bg-foreground px-2 py-1 text-xs text-background shadow-md">
                        {deleteTip}
                        <Tooltip.Arrow className="fill-foreground" />
                      </Tooltip.Content>
                    </Tooltip.Portal>
                  </Tooltip.Root>
                </div>
              </CardContent>
            </Card>
            )
          })}
        </div>
      </div>
    </Tooltip.Provider>
  )
}
