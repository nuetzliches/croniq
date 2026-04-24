import { useState } from 'react'
import { useForm } from 'react-hook-form'
import * as Dialog from '@radix-ui/react-dialog'
import { Plus, Trash2, X, CalendarDays } from 'lucide-react'
import { useCalendars, useCreateCalendar, useDeleteCalendar } from '@/api/hooks'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { EmptyState } from '@/components/ui/empty-state'
import { Spinner } from '@/components/ui/spinner'

interface CalendarForm {
  name: string
  timezone: string
  rules: string
}

export function CalendarsPage() {
  const { data: calendars, isLoading } = useCalendars()
  const createCalendar = useCreateCalendar()
  const deleteCalendar = useDeleteCalendar()
  const [open, setOpen] = useState(false)
  const [rulesError, setRulesError] = useState<string | null>(null)

  const { register, handleSubmit, reset, formState: { errors } } = useForm<CalendarForm>()

  const inputCls = 'w-full px-3 py-2 border border-border rounded-md text-sm bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary'

  function resetDialog() {
    reset()
    setRulesError(null)
  }

  async function onSubmit(data: CalendarForm) {
    setRulesError(null)
    try {
      await createCalendar.mutateAsync({
        name: data.name,
        timezone: data.timezone || undefined,
        rules: data.rules || undefined,
      })
    } catch (e) {
      // apiFetch throws `Error("${status}: ${body}")` — body is JSON
      // `{ error, message }` on validation failures.
      const msg = e instanceof Error ? e.message : String(e)
      const match = msg.match(/^(\d+):\s*(.+)$/s)
      if (match && match[1] === '400') {
        try {
          const parsed = JSON.parse(match[2])
          setRulesError(parsed.message ?? 'Invalid calendar rules')
        } catch {
          setRulesError(match[2] || 'Invalid calendar rules')
        }
        return
      }
      setRulesError(msg)
      return
    }
    resetDialog()
    setOpen(false)
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">{calendars?.length ?? 0} calendars defined</p>
        <Dialog.Root open={open} onOpenChange={(v) => { setOpen(v); if (!v) resetDialog() }}>
          <Dialog.Trigger asChild>
            <Button size="sm"><Plus className="h-3.5 w-3.5" />Add Calendar</Button>
          </Dialog.Trigger>
          <Dialog.Portal>
            <Dialog.Overlay className="fixed inset-0 z-40 bg-black/40" />
            <Dialog.Content className="fixed left-1/2 top-1/2 z-50 w-full max-w-md -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-card p-6 shadow-xl">
              <div className="flex items-center justify-between mb-4">
                <Dialog.Title className="text-sm font-semibold">Add Calendar</Dialog.Title>
                <Dialog.Close className="text-muted-foreground hover:text-foreground">
                  <X className="h-4 w-4" />
                </Dialog.Close>
              </div>
              <form onSubmit={handleSubmit(onSubmit)} className="space-y-3">
                <div>
                  <input {...register('name', { required: 'Required' })} placeholder="Calendar name (e.g. eu-business-hours)" className={inputCls} />
                  {errors.name && <p className="text-xs text-destructive mt-1">{errors.name.message}</p>}
                </div>
                <input {...register('timezone')} placeholder="Timezone (e.g. Europe/Vienna)" className={inputCls} />
                <div>
                  <textarea
                    {...register('rules')}
                    placeholder={'Rules (optional DSL)\ne.g. include weekly "Mon".."Fri"\ninclude window "08:00".."18:00"'}
                    rows={4}
                    className={`${inputCls} resize-none font-mono`}
                    onChange={(e) => { register('rules').onChange(e); if (rulesError) setRulesError(null) }}
                  />
                  {rulesError && (
                    <p className="text-xs text-destructive mt-1 whitespace-pre-wrap">{rulesError}</p>
                  )}
                </div>
                <div className="flex justify-end gap-2 pt-2">
                  <Dialog.Close asChild><Button variant="secondary" size="sm" type="button">Cancel</Button></Dialog.Close>
                  <Button type="submit" size="sm" disabled={createCalendar.isPending}>
                    {createCalendar.isPending ? <><Spinner className="h-3.5 w-3.5" />Saving…</> : 'Save Calendar'}
                  </Button>
                </div>
              </form>
            </Dialog.Content>
          </Dialog.Portal>
        </Dialog.Root>
      </div>

      {isLoading && <div className="flex justify-center py-12"><Spinner className="h-6 w-6" /></div>}

      {!isLoading && calendars?.length === 0 && (
        <EmptyState
          icon={<CalendarDays className="h-10 w-10" />}
          title="No calendars defined"
          description="Calendars let you restrict job execution to specific windows and working days"
          action={<Button size="sm" onClick={() => setOpen(true)}><Plus className="h-3.5 w-3.5" />Add Calendar</Button>}
        />
      )}

      <div className="space-y-2">
        {calendars?.map((cal) => (
          <Card key={cal.calendar_id}>
            <CardContent className="py-3">
              <div className="flex items-center gap-4">
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="font-mono text-sm font-medium">{cal.name}</span>
                    {cal.timezone && (
                      <span className="text-xs text-muted-foreground">{cal.timezone}</span>
                    )}
                  </div>
                  {cal.rules && (
                    <pre className="text-xs text-muted-foreground mt-1 font-mono truncate">{cal.rules.slice(0, 80)}{cal.rules.length > 80 ? '…' : ''}</pre>
                  )}
                  <p className="text-xs text-muted-foreground mt-1">Created {new Date(cal.created_at).toLocaleDateString()}</p>
                </div>
                <Button
                  variant="ghost" size="sm"
                  onClick={() => deleteCalendar.mutate(cal.calendar_id)}
                  aria-label={`Delete calendar ${cal.name}`}
                  className="h-7 w-7 p-0 text-muted-foreground hover:text-destructive shrink-0"
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>
    </div>
  )
}
