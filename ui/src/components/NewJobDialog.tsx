import { useEffect } from 'react'
import { useForm } from 'react-hook-form'
import { useNavigate } from 'react-router'
import * as Dialog from '@radix-ui/react-dialog'
import { X, AlertCircle } from 'lucide-react'
import { useCreateJob } from '@/api/hooks'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
}

interface NewJobForm {
  job_key: string
  description: string
  timeout: string
  tags: string
  dead_letter_enabled: boolean
  dead_letter_retention: string
  dead_letter_operator_hint: string
  dead_letter_replay_max_age: string
}

const inputCls =
  'w-full px-3 py-2 border border-border rounded-md text-sm bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary'

// Convention enforced by the Croniqfile parser: `namespace:name` or
// `namespace:name:variant`. The HTTP endpoint itself is permissive, but
// the UI gates on the same shape so newly-created jobs round-trip
// cleanly into a Croniqfile if the operator later adopts DSL ownership.
const JOB_KEY_RE = /^[A-Za-z0-9_-]+:[A-Za-z0-9_-]+(:[A-Za-z0-9_-]+)?$/

export function NewJobDialog({ open, onOpenChange }: Props) {
  const navigate = useNavigate()
  const createJob = useCreateJob()
  const {
    register,
    handleSubmit,
    reset,
    watch,
    formState: { errors },
  } = useForm<NewJobForm>()
  const deadLetterOn = watch('dead_letter_enabled')

  // Re-seed the form whenever the dialog opens. `createJob.error` is left
  // alone here — the `useMutation` object is a new reference on every
  // render, so putting it in deps would re-fire the effect and loop. Any
  // stale error clears on the next submit.
  useEffect(() => {
    if (open) {
      reset({
        job_key: '',
        description: '',
        timeout: '',
        tags: '',
        dead_letter_enabled: true,
        dead_letter_retention: '',
        dead_letter_operator_hint: '',
        dead_letter_replay_max_age: '',
      })
    }
  }, [open, reset])

  async function onSubmit(data: NewJobForm) {
    const tags = data.tags
      .split(',')
      .map((t) => t.trim())
      .filter((t) => t.length > 0)
    const job = await createJob.mutateAsync({
      job_key: data.job_key.trim(),
      description: data.description.trim() === '' ? null : data.description,
      timeout: data.timeout.trim() === '' ? null : data.timeout,
      tags,
      dead_letter_enabled: data.dead_letter_enabled,
      dead_letter_retention:
        data.dead_letter_retention.trim() === '' ? null : data.dead_letter_retention.trim(),
      dead_letter_operator_hint:
        data.dead_letter_operator_hint.trim() === '' ? null : data.dead_letter_operator_hint,
      dead_letter_replay_max_age:
        data.dead_letter_replay_max_age.trim() === '' ? null : data.dead_letter_replay_max_age.trim(),
    })
    onOpenChange(false)
    navigate(`/jobs/${encodeURIComponent(job.job_key)}`)
  }

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/40" />
        <Dialog.Content className="fixed left-1/2 top-1/2 z-50 w-full max-w-md -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-card p-6 shadow-xl">
          <div className="flex items-center justify-between mb-4">
            <Dialog.Title className="text-sm font-semibold">New Job</Dialog.Title>
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
                Job key
              </label>
              <input
                {...register('job_key', {
                  required: 'Job key is required',
                  pattern: {
                    value: JOB_KEY_RE,
                    message: 'Use namespace:name or namespace:name:variant (letters, digits, _ -).',
                  },
                })}
                autoFocus
                placeholder="namespace:name"
                className={inputCls}
              />
              {errors.job_key ? (
                <p className="text-xs text-destructive mt-1">{errors.job_key.message}</p>
              ) : (
                <p className="text-[11px] text-muted-foreground mt-1">
                  Identity of the job. Cannot be changed later — pick a stable, readable key.
                </p>
              )}
            </div>
            <div>
              <label className="block text-xs text-muted-foreground mb-1">
                Description
              </label>
              <input
                {...register('description')}
                placeholder="Short summary of what the job does"
                className={inputCls}
              />
            </div>
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
                How long an execution may run before being killed. Empty for the server default.
              </p>
            </div>
            <div>
              <label className="block text-xs text-muted-foreground mb-1">
                Tags
              </label>
              <input
                {...register('tags')}
                placeholder="env=prod, team=ops, owner=alice"
                className={inputCls}
              />
              <p className="text-[11px] text-muted-foreground mt-1">
                Comma-separated free-form tags for filtering. Convention: <code>key=value</code>.
              </p>
            </div>
            <div>
              <label className="flex items-center gap-2 text-xs text-foreground">
                <input type="checkbox" {...register('dead_letter_enabled')} className="h-3.5 w-3.5" />
                Dead-lettering enabled
              </label>
              <p className="text-[11px] text-muted-foreground mt-1">
                When on, an execution that exhausts its retries is kept in the dead-letter queue for triage. Turn off to drop permanently-failed executions instead.
              </p>
            </div>
            {deadLetterOn && (
              <div className="space-y-3 border-l-2 border-border pl-3">
                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="block text-xs text-muted-foreground mb-1">
                      Retention
                    </label>
                    <input
                      {...register('dead_letter_retention')}
                      placeholder="e.g. 30d, 0 = forever"
                      className={inputCls}
                    />
                  </div>
                  <div>
                    <label className="block text-xs text-muted-foreground mb-1">
                      Replay max age
                    </label>
                    <input
                      {...register('dead_letter_replay_max_age')}
                      placeholder="e.g. 7d"
                      className={inputCls}
                    />
                  </div>
                </div>
                <p className="text-[11px] text-muted-foreground -mt-1">
                  Replay max age is an opt-in guard: replaying a dead letter originally scheduled longer ago than this is rejected unless forced. Empty for no guard; retention empty for the server default (30d).
                </p>
                <div>
                  <label className="block text-xs text-muted-foreground mb-1">
                    Operator hint
                  </label>
                  <input
                    {...register('dead_letter_operator_hint')}
                    placeholder="e.g. Re-run the nightly export before replaying"
                    className={inputCls}
                  />
                  <p className="text-[11px] text-muted-foreground mt-1">
                    Free-form triage note shown alongside this job's dead letters.
                  </p>
                </div>
              </div>
            )}
            {createJob.error && (
              <p className="text-xs text-destructive flex items-center gap-1">
                <AlertCircle className="h-3.5 w-3.5" />
                {String(createJob.error)}
              </p>
            )}
            <div className="flex justify-end gap-2 pt-2">
              <Dialog.Close asChild>
                <Button variant="secondary" size="sm" type="button">Cancel</Button>
              </Dialog.Close>
              <Button type="submit" size="sm" disabled={createJob.isPending}>
                {createJob.isPending ? <><Spinner className="h-3.5 w-3.5" />Creating…</> : 'Create job'}
              </Button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
