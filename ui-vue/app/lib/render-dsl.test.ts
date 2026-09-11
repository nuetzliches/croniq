import { describe, expect, it } from 'vitest'
import { renderDsl } from './render-dsl'
import type { CalendarDefinition, JobDefinition, TriggerDefinition } from '~/api/types'

/**
 * The renderer emits text an operator may paste into a Croniqfile, so these
 * assert the shape of the output rather than that it "contains" something —
 * a dropped field or a mis-quoted string is a real defect here.
 */

const job: JobDefinition = {
  job_key: 'demo:report',
  description: 'Nightly report',
  assigned_runner_id: null,
  is_active: true,
  metadata: {},
  created_at: '2026-09-01T00:00:00Z',
  updated_at: '2026-09-01T00:00:00Z',
  timeout: '10m',
  max_retries: 3,
  dead_letter_enabled: true,
  dead_letter_retention: null,
  dead_letter_operator_hint: null,
  dead_letter_replay_max_age: null,
  tags: ['nightly', 'report'],
}

const trigger: TriggerDefinition = {
  trigger_id: 't1',
  job_key: 'demo:report',
  cron_expression: '0 3 * * *',
  timezone: 'Europe/Berlin',
  calendar: null,
  window: null,
  enabled: true,
  managed_by: 'api',
  created_at: '2026-09-01T00:00:00Z',
  updated_at: '2026-09-01T00:00:00Z',
}

describe('renderDsl', () => {
  it('renders a job with its schedule', () => {
    const out = renderDsl(job, [trigger], [])
    expect(out).toContain('job "demo:report" {')
    expect(out).toContain('  description = "Nightly report"')
    expect(out).toContain('  tags        = ["nightly","report"]')
    expect(out).toContain('  timeout     = "10m"')
    expect(out).toContain('  max_retries = 3')
    expect(out).toContain('    rule = "0 3 * * *"')
    expect(out).toContain('    tz   = "Europe/Berlin"')
  })

  it('quotes a description containing a quote rather than breaking the block', () => {
    const out = renderDsl({ ...job, description: 'say "hi"' }, [], [])
    expect(out).toContain('  description = "say \\"hi\\""')
  })

  it('defaults a missing timeout to the DSL default rather than emitting null', () => {
    const out = renderDsl({ ...job, timeout: null }, [], [])
    expect(out).toContain('  timeout     = "5m"')
    expect(out).not.toContain('null')
  })

  it('omits max_retries when the job does not set one', () => {
    expect(renderDsl({ ...job, max_retries: null }, [], [])).not.toContain('max_retries')
  })

  it('emits the dead_letter block only when it is switched off', () => {
    expect(renderDsl(job, [], [])).not.toContain('dead_letter')
    expect(renderDsl({ ...job, dead_letter_enabled: false }, [], [])).toContain(
      '  dead_letter { enabled = false }',
    )
  })

  it('marks a disabled schedule instead of rendering it as live', () => {
    const out = renderDsl(job, [{ ...trigger, enabled: false }], [])
    expect(out).toContain('  # this schedule is currently disabled')
  })

  it('surfaces further triggers as comments, since a job block holds one', () => {
    const second: TriggerDefinition = {
      ...trigger,
      trigger_id: 't2',
      cron_expression: '0 15 * * MON',
      timezone: null,
      window: '30m',
      enabled: false,
    }
    const out = renderDsl(job, [trigger, second], [])
    expect(out).toContain('  # +1 more schedule attached via API')
    expect(out).toContain('  #   rule "0 15 * * MON" · window 30m · (disabled)')
  })

  it('inlines the referenced calendar when it resolves', () => {
    const calendar: CalendarDefinition = {
      calendar_id: 'c1',
      name: 'business-days',
      timezone: 'Europe/Berlin',
      rules: 'exclude weekends\nexclude "2026-12-25"',
      managed_by: 'api',
      created_at: '2026-09-01T00:00:00Z',
      updated_at: '2026-09-01T00:00:00Z',
    }
    const out = renderDsl(job, [{ ...trigger, calendar: 'business-days' }], [calendar])
    expect(out).toContain('calendar "business-days" {')
    expect(out).toContain('  timezone "Europe/Berlin"')
    expect(out).toContain('  exclude weekends')
    expect(out).toContain('  exclude "2026-12-25"')
  })

  it('reports an unresolved calendar reference once the list has loaded', () => {
    const out = renderDsl(job, [{ ...trigger, calendar: 'gone' }], [])
    expect(out).toContain('# calendar "gone" is referenced but could not be resolved')
  })

  it('stays quiet about a reference while the calendar list is still loading', () => {
    const out = renderDsl(job, [{ ...trigger, calendar: 'gone' }], undefined)
    expect(out).not.toContain('could not be resolved')
  })
})
