import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { CalendarDefinition, JobDefinition, TriggerDefinition } from '~/api/types'

/**
 * The wasm bridge is mocked here, deliberately and with a clear division of
 * labour.
 *
 * Whether the emitted text *parses* is not this file's business: the Rust side
 * parses its own output before returning it, so that guarantee holds by
 * construction and is checked end-to-end against `croniq validate` instead
 * (ui/scripts/dsl-parses.mjs).
 *
 * What is this file's business is everything around the call — which fields
 * reach the formatter, and whether the notes tell the truth about what could
 * not be carried across. That is the part a hand-written renderer got wrong
 * for as long as it existed.
 */
vi.mock('~/lib/croniq-dsl', () => ({
  formatJobBlock: vi.fn(async (payload, key, options) =>
    `job ${key} { ${JSON.stringify({ payload, options })} }`,
  ),
  formatCalendarBlock: vi.fn(async (rules, name) => `calendar ${name} { ${rules.length} rules }`),
  parseSchedule: vi.fn(async (dsl: string) =>
    dsl.startsWith('every')
      ? { ok: true, schedule: { mode: 'interval', count: 15, unit: 'minutes' }, error: null }
      : { ok: false, schedule: null, error: 'not a schedule' },
  ),
  parseCalendarRules: vi.fn(async () => ({
    ok: true,
    rules: [{ action: 'include', rule_type: 'weekly', args: ['Mon'] }],
    diagnostics: [],
  })),
}))

const { renderJobDsl } = await import('./render-dsl')
const dsl = await import('~/lib/croniq-dsl')

const job: JobDefinition = {
  job_key: 'demo:report',
  description: 'Nightly report',
  assigned_runner_id: null,
  is_active: true,
  metadata: {},
  created_at: '2026-09-01T00:00:00Z',
  updated_at: '2026-09-01T00:00:00Z',
  timeout: '10m',
  max_retries: null,
  dead_letter_enabled: null,
  dead_letter_retention: null,
  dead_letter_operator_hint: null,
  dead_letter_replay_max_age: null,
  tags: ['nightly', 'report'],
}

const trigger: TriggerDefinition = {
  trigger_id: 't1',
  job_key: 'demo:report',
  cron_expression: 'every 15 minutes',
  timezone: 'Europe/Berlin',
  calendar: null,
  window: null,
  enabled: true,
  managed_by: 'api',
  created_at: '2026-09-01T00:00:00Z',
  updated_at: '2026-09-01T00:00:00Z',
}

/** The options object handed to the formatter on the last call. */
function lastOptions() {
  const calls = vi.mocked(dsl.formatJobBlock).mock.calls
  return calls[calls.length - 1]![2] as Record<string, unknown>
}

describe('renderJobDsl', () => {
  beforeEach(() => vi.clearAllMocks())

  it('passes the job fields the API records through to the formatter', async () => {
    const result = await renderJobDsl(job, [trigger], [])
    expect(result.text).toContain('job demo:report')
    expect(lastOptions()).toMatchObject({
      description: 'Nightly report',
      timeout: '10m',
      tags: ['nightly', 'report'],
      schedule_timezone: 'Europe/Berlin',
    })
    expect(result.notes).toEqual([])
  })

  it('never emits a retry block, because the API does not record a strategy', async () => {
    const result = await renderJobDsl({ ...job, max_retries: 3 }, [trigger], [])
    expect(lastOptions()).not.toHaveProperty('retry')
    expect(result.notes.join(' ')).toContain('retries 3 times')
    expect(result.notes.join(' ')).toContain('does not record which backoff strategy')
  })

  it('sends a dead-letter block only when the job departs from the default', async () => {
    await renderJobDsl(job, [trigger], [])
    expect(lastOptions().dead_letter).toBeUndefined()

    await renderJobDsl({ ...job, dead_letter_retention: '60d' }, [trigger], [])
    expect(lastOptions().dead_letter).toMatchObject({ retention: '60d' })
  })

  it('renders a job with no trigger as disabled rather than as nothing', async () => {
    const result = await renderJobDsl(job, [], [])
    expect(vi.mocked(dsl.formatJobBlock).mock.calls[0]![0]).toEqual({ mode: 'disabled' })
    expect(result.text).toContain('job demo:report')
  })

  it('refuses to render a schedule the DSL cannot express, and says which', async () => {
    const result = await renderJobDsl(job, [{ ...trigger, cron_expression: '*/10 * * * *' }], [])
    expect(result.text).toBe('')
    expect(result.notes.join(' ')).toContain('*/10 * * * *')
    expect(result.notes.join(' ')).toContain('not raw cron')
    expect(dsl.formatJobBlock).not.toHaveBeenCalled()
  })

  it('reports further triggers instead of silently dropping them', async () => {
    const second = { ...trigger, trigger_id: 't2' }
    const result = await renderJobDsl(job, [trigger, second], [])
    expect(result.notes.join(' ')).toContain('1 further schedule')
    expect(result.notes.join(' ')).toContain('A job block holds one')
  })

  it('inlines a referenced calendar so the text stands on its own', async () => {
    const calendar: CalendarDefinition = {
      calendar_id: 'c1',
      name: 'business-days',
      timezone: 'Europe/Berlin',
      rules: 'include weekly weekday',
      managed_by: 'api',
      created_at: '2026-09-01T00:00:00Z',
      updated_at: '2026-09-01T00:00:00Z',
    }
    const result = await renderJobDsl(
      job,
      [{ ...trigger, calendar: 'business-days' }],
      [calendar],
    )
    expect(lastOptions().schedule_calendar).toBe('business-days')
    expect(result.text).toContain('calendar business-days')
    expect(result.notes).toEqual([])
  })

  it('says so when the referenced calendar does not exist', async () => {
    const result = await renderJobDsl(job, [{ ...trigger, calendar: 'gone' }], [])
    expect(result.notes.join(' ')).toContain('no such calendar exists')
    expect(result.notes.join(' ')).toContain('would fail to load')
  })

  it('stays quiet about a reference while the calendar list is still loading', async () => {
    const result = await renderJobDsl(job, [{ ...trigger, calendar: 'gone' }], undefined)
    expect(result.notes).toEqual([])
    expect(dsl.formatCalendarBlock).not.toHaveBeenCalled()
  })

  it('reports a formatter refusal rather than rendering half a job', async () => {
    vi.mocked(dsl.formatJobBlock).mockRejectedValueOnce(new Error('invalid job key'))
    const result = await renderJobDsl(job, [trigger], [])
    expect(result.text).toBe('')
    expect(result.notes.join(' ')).toContain('invalid job key')
  })
})
