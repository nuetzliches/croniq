import { useCallback, useState } from 'react'
import { useForm } from 'react-hook-form'
import * as Dialog from '@radix-ui/react-dialog'
import * as Tooltip from '@radix-ui/react-tooltip'
import { Plus, Pencil, Trash2, X, CalendarDays, Download } from 'lucide-react'
import {
  useCalendars,
  useCreateCalendar,
  useUpdateCalendar,
  useDeleteCalendar,
  useAdoptCalendar,
} from '@/api/hooks'
import type { CalendarDefinition } from '@/api/types'
import { Badge } from '@/components/ui/badge'
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
  const updateCalendar = useUpdateCalendar()
  const deleteCalendar = useDeleteCalendar()
  const adoptCalendar = useAdoptCalendar()
  const { confirm, dialog: confirmDialog } = useConfirm()
  const [adoptError, setAdoptError] = useState<string | null>(null)
  const [open, setOpen] = useState(false)
  const [rulesError, setRulesError] = useState<string | null>(null)
  // null → create mode, set → edit mode (form is seeded with the row's
  // current name/timezone/rules and submit calls PUT).
  const [editingCalendar, setEditingCalendar] = useState<CalendarDefinition | null>(null)

  async function handleAdopt(cal: { calendar_id: string; name: string }) {
    const ok = await confirm({
      title: `Adopt calendar ${cal.name}?`,
      description:
        'A copy of this calendar is created in the API store and the Croniqfile definition is ignored on the next reload until you unadopt. Requires `policy { dsl_adopt_on_mutate true }` in the Croniqfile.',
      confirmLabel: 'Adopt to edit',
    })
    if (!ok) return
    setAdoptError(null)
    try {
      await adoptCalendar.mutateAsync(cal.calendar_id)
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      // 409 carries `{error, message}` JSON; surface the message.
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
    reset({ name: '', timezone: '', rules: '' })
    setRulesError(null)
    setRulesMode('builder')
    setEditingCalendar(null)
  }

  function openEdit(cal: CalendarDefinition) {
    reset({
      name: cal.name,
      timezone: cal.timezone ?? '',
      rules: cal.rules ?? '',
    })
    // The builder's `initial` would let us round-trip into form state,
    // but parsing a stored DSL string back into the typed payload is
    // best-effort. Default to "advanced" — the saved DSL is the most
    // accurate representation we have.
    setRulesMode('advanced')
    setRulesError(null)
    setEditingCalendar(cal)
    setOpen(true)
  }

  async function onSubmit(data: CalendarForm) {
    setRulesError(null)
    try {
      if (editingCalendar) {
        await updateCalendar.mutateAsync({
          calendar_id: editingCalendar.calendar_id,
          name: data.name,
          // Empty string clears the override — same convention as the
          // schedule update endpoint.
          timezone: data.timezone ?? '',
          rules: data.rules ?? '',
        })
        resetDialog()
        setOpen(false)
        return
      }
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
    <div className="page wide">
      {confirmDialog}
      <div className="page-head">
        <div>
          <h1 className="page-title">Calendars</h1>
          <p className="page-subtitle">{calendars?.length ?? 0} calendars defined · attach to jobs to gate firing.</p>
        </div>
        <Dialog.Root open={open} onOpenChange={(v) => { setOpen(v); if (!v) resetDialog() }}>
          <Dialog.Trigger asChild>
            <Button size="sm"><Plus className="h-3.5 w-3.5" />Add Calendar</Button>
          </Dialog.Trigger>
          <Dialog.Portal>
            <Dialog.Overlay className="fixed inset-0 z-40 bg-black/40" />
            <Dialog.Content className="fixed left-1/2 top-1/2 z-50 w-full max-w-md -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-card p-6 shadow-xl">
              <div className="flex items-center justify-between mb-4">
                <Dialog.Title className="text-sm font-semibold">
                  {editingCalendar ? `Edit Calendar — ${editingCalendar.name}` : 'Add Calendar'}
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
                  <Button
                    type="submit"
                    size="sm"
                    disabled={createCalendar.isPending || updateCalendar.isPending}
                  >
                    {(createCalendar.isPending || updateCalendar.isPending) ? (
                      <><Spinner className="h-3.5 w-3.5" />Saving…</>
                    ) : editingCalendar ? 'Save Changes' : 'Save Calendar'}
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

      {adoptError && (
        <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
          {adoptError}
        </div>
      )}
      <Tooltip.Provider delayDuration={200}>
        <div className="space-y-2">
          {calendars?.map((cal) => {
            const isDsl = cal.managed_by === 'dsl'
            const dslTip =
              'Managed by the Croniqfile — edit the file to change, or click the adopt button to copy it into the API store.'
            return (
              <Card key={cal.calendar_id}>
                <CardContent className="py-3">
                  <div className="flex items-center gap-4">
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="font-mono text-sm font-medium">{cal.name}</span>
                        {cal.timezone && (
                          <span className="text-xs text-muted-foreground">{cal.timezone}</span>
                        )}
                        {isDsl && (
                          <Badge variant="neutral" className="font-mono">dsl</Badge>
                        )}
                      </div>
                      {cal.rules && (
                        <pre className="text-xs text-muted-foreground mt-1 font-mono truncate">{cal.rules.slice(0, 80)}{cal.rules.length > 80 ? '…' : ''}</pre>
                      )}
                      <p className="text-xs text-muted-foreground mt-1">
                        Created <RelativeTime iso={cal.created_at} />
                      </p>
                    </div>
                    <div className="flex items-center gap-0.5 shrink-0">
                      {isDsl && (
                        <Tooltip.Root>
                          <Tooltip.Trigger asChild>
                            <span className="inline-flex">
                              <Button
                                variant="ghost" size="sm"
                                onClick={() => handleAdopt(cal)}
                                disabled={adoptCalendar.isPending}
                                aria-label={`Adopt calendar ${cal.name}`}
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
                      <Tooltip.Root>
                        <Tooltip.Trigger asChild>
                          <span className="inline-flex">
                            <Button
                              variant="ghost" size="sm"
                              onClick={() => openEdit(cal)}
                              disabled={isDsl}
                              aria-label={`Edit calendar ${cal.name}`}
                              className="h-7 w-7 p-0 text-muted-foreground hover:text-primary disabled:cursor-not-allowed"
                            >
                              <Pencil className="h-3.5 w-3.5" />
                            </Button>
                          </span>
                        </Tooltip.Trigger>
                        {isDsl && (
                          <Tooltip.Portal>
                            <Tooltip.Content className="z-50 max-w-xs rounded-md bg-foreground px-2 py-1 text-xs text-background shadow-md">
                              {dslTip}
                              <Tooltip.Arrow className="fill-foreground" />
                            </Tooltip.Content>
                          </Tooltip.Portal>
                        )}
                      </Tooltip.Root>
                      <Tooltip.Root>
                        <Tooltip.Trigger asChild>
                          <span className="inline-flex">
                            <Button
                              variant="ghost" size="sm"
                              onClick={() => handleDelete(cal)}
                              disabled={isDsl}
                              aria-label={`Delete calendar ${cal.name}`}
                              className="h-7 w-7 p-0 text-muted-foreground hover:text-destructive disabled:cursor-not-allowed"
                            >
                              <Trash2 className="h-3.5 w-3.5" />
                            </Button>
                          </span>
                        </Tooltip.Trigger>
                        {isDsl && (
                          <Tooltip.Portal>
                            <Tooltip.Content className="z-50 max-w-xs rounded-md bg-foreground px-2 py-1 text-xs text-background shadow-md">
                              {dslTip}
                              <Tooltip.Arrow className="fill-foreground" />
                            </Tooltip.Content>
                          </Tooltip.Portal>
                        )}
                      </Tooltip.Root>
                    </div>
                  </div>
                </CardContent>
              </Card>
            )
          })}
        </div>
      </Tooltip.Provider>
    </div>
  )
}
