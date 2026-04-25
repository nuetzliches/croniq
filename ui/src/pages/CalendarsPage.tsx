import { useCallback, useState } from 'react'
import { useForm } from 'react-hook-form'
import * as Dialog from '@radix-ui/react-dialog'
import { Plus, Trash2, X, CalendarDays } from 'lucide-react'
import { useCalendars, useCreateCalendar, useDeleteCalendar } from '@/api/hooks'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { EmptyState } from '@/components/ui/empty-state'
import { Spinner } from '@/components/ui/spinner'
import { useConfirm } from '@/components/ui/confirm-dialog'
import { RelativeTime } from '@/components/ui/relative-time'
import { CalendarRuleBuilder } from '@/components/builders/CalendarRuleBuilder'
import { TimezoneInput } from '@/components/ui/timezone-input'

interface CalendarForm {
  name: string
  timezone: string
  rules: string
}

export function CalendarsPage() {
  const { data: calendars, isLoading } = useCalendars()
  const createCalendar = useCreateCalendar()
  const deleteCalendar = useDeleteCalendar()
  const { confirm, dialog: confirmDialog } = useConfirm()
  const [open, setOpen] = useState(false)
  const [rulesError, setRulesError] = useState<string | null>(null)

  async function handleDelete(cal: { calendar_id: string; name: string }) {
    const ok = await confirm({
      title: `Delete calendar ${cal.name}?`,
      description:
        'Jobs that reference this calendar by name will fail to load on the next config reload. Existing executions are unaffected.',
      confirmLabel: 'Delete calendar',
      destructive: true,
    })
    if (ok) deleteCalendar.mutate(cal.calendar_id)
  }

  const { register, handleSubmit, reset, setValue, formState: { errors } } = useForm<CalendarForm>()

  const inputCls = 'w-full px-3 py-2 border border-border rounded-md text-sm bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary'

  // Two ways to enter the rules:
  //   "builder"  — interactive form, drives `rules` via wasm
  //   "advanced" — raw textarea (the original UX, kept as escape hatch)
  // Server still receives a single DSL string in `rules`.
  const [rulesMode, setRulesMode] = useState<'builder' | 'advanced'>('builder')
  const onBuilderChange = useCallback(
    (dsl: string) => setValue('rules', dsl),
    [setValue],
  )
  const onBuilderError = useCallback((msg: string | null) => setRulesError(msg), [])

  function resetDialog() {
    reset()
    setRulesError(null)
    setRulesMode('builder')
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
      {confirmDialog}
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
                <Dialog.Close
                  aria-label="Close dialog"
                  className="text-muted-foreground hover:text-foreground"
                >
                  <X className="h-4 w-4" />
                </Dialog.Close>
              </div>
              <form onSubmit={handleSubmit(onSubmit)} className="space-y-3">
                <div>
                  <input {...register('name', { required: 'Required' })} placeholder="Calendar name (e.g. eu-business-hours)" className={inputCls} />
                  {errors.name && <p className="text-xs text-destructive mt-1">{errors.name.message}</p>}
                </div>
                <TimezoneInput
                  {...register('timezone')}
                  className={inputCls}
                  showDetectedHint
                />
                <div>
                  <div className="flex items-center justify-between mb-1.5">
                    <label className="text-xs font-medium text-foreground">Rules (optional)</label>
                    {/* Mode toggle — Builder is the default, Advanced is
                        the escape hatch for power users editing existing
                        calendars in raw DSL. */}
                    <div role="tablist" className="inline-flex border border-border rounded-md p-0.5 text-[11px]">
                      {(['builder', 'advanced'] as const).map((m) => (
                        <button
                          key={m}
                          type="button"
                          role="tab"
                          aria-selected={rulesMode === m}
                          onClick={() => setRulesMode(m)}
                          className={`px-2 py-0.5 rounded-sm capitalize ${
                            rulesMode === m
                              ? 'bg-primary/15 text-primary'
                              : 'text-muted-foreground hover:text-foreground'
                          }`}
                        >
                          {m === 'builder' ? 'Builder' : 'Advanced (raw)'}
                        </button>
                      ))}
                    </div>
                  </div>
                  {rulesMode === 'builder' ? (
                    <>
                      <CalendarRuleBuilder onChange={onBuilderChange} onError={onBuilderError} />
                      {/* Hidden RHF field — the builder writes here so
                          submit picks up the produced DSL string. */}
                      <input type="hidden" {...register('rules')} />
                    </>
                  ) : (
                    <textarea
                      {...register('rules')}
                      placeholder={'Croniqfile DSL — leave empty for "always on"'}
                      rows={4}
                      className={`${inputCls} resize-none font-mono`}
                      onChange={(e) => {
                        register('rules').onChange(e)
                        if (rulesError) setRulesError(null)
                      }}
                    />
                  )}
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
                  <p className="text-xs text-muted-foreground mt-1">
                    Created <RelativeTime iso={cal.created_at} />
                  </p>
                </div>
                <Button
                  variant="ghost" size="sm"
                  onClick={() => handleDelete(cal)}
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
