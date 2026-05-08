import { useEffect } from 'react'
import { useForm } from 'react-hook-form'
import * as Dialog from '@radix-ui/react-dialog'
import { X, AlertCircle } from 'lucide-react'
import { useUpdateJob } from '@/api/hooks'
import type { JobDefinition } from '@/api/types'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'

interface Props {
  job: JobDefinition | null
  open: boolean
  onOpenChange: (open: boolean) => void
}

interface EditForm {
  description: string
  timeout: string
  tags: string
}

const inputCls =
  'w-full px-3 py-2 border border-border rounded-md text-sm bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary'

/// Edit dialog for a job's mutable metadata. `job_key` is the identity and
/// stays read-only — schedule and activation each have their own dialogs.
/// DSL-managed jobs are filtered out by the caller; we still surface the
/// 409 in the inline error for safety.
export function EditJobDialog({ job, open, onOpenChange }: Props) {
  const updateJob = useUpdateJob()
  const { register, handleSubmit, reset, formState: { errors } } = useForm<EditForm>()

  // Re-seed the form when the dialog opens with a (possibly different)
  // job. `useForm`'s defaultValues only apply at mount, so RHF needs an
  // explicit reset to pick up the new row.
  useEffect(() => {
    if (open && job) {
      reset({
        description: job.description ?? '',
        timeout: job.timeout ?? '',
        tags: (job.tags ?? []).join(', '),
      })
    }
  }, [open, job, reset])

  async function onSubmit(data: EditForm) {
    if (!job) return
    const tags = data.tags
      .split(',')
      .map((t) => t.trim())
      .filter((t) => t.length > 0)
    await updateJob.mutateAsync({
      job_key: job.job_key,
      // Empty string clears the field — send explicit null so the backend
      // overwrites with NULL instead of treating an empty string as a
      // value.
      description: data.description.trim() === '' ? null : data.description,
      timeout: data.timeout.trim() === '' ? null : data.timeout,
      tags,
    })
    onOpenChange(false)
  }

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/40" />
        <Dialog.Content className="fixed left-1/2 top-1/2 z-50 w-full max-w-md -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-card p-6 shadow-xl">
          <div className="flex items-center justify-between mb-4">
            <Dialog.Title className="text-sm font-semibold">
              Edit Job{job ? ` — ${job.job_key}` : ''}
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
                Description
              </label>
              <input
                {...register('description')}
                placeholder="Short summary of what the job does"
                className={inputCls}
              />
              {errors.description && (
                <p className="text-xs text-destructive mt-1">{errors.description.message}</p>
              )}
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
                Comma-separated free-form tags for filtering. Convention: <code>key=value</code>. Not routing-relevant.
              </p>
            </div>
            {updateJob.error && (
              <p className="text-xs text-destructive flex items-center gap-1">
                <AlertCircle className="h-3.5 w-3.5" />
                {String(updateJob.error)}
              </p>
            )}
            <div className="flex justify-end gap-2 pt-2">
              <Dialog.Close asChild>
                <Button variant="secondary" size="sm" type="button">Cancel</Button>
              </Dialog.Close>
              <Button type="submit" size="sm" disabled={updateJob.isPending}>
                {updateJob.isPending ? <><Spinner className="h-3.5 w-3.5" />Saving…</> : 'Save'}
              </Button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
