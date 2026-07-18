import { describe, expect, it } from 'vitest'
import { renderDsl } from './render-dsl'
import type { CalendarDefinition, JobDefinition, TriggerDefinition } from '@/api/types'

function makeJob(overrides: Partial<JobDefinition> = {}): JobDefinition {
  return {
    job_key: 'nightly-report',
    description: 'Nightly report',
    assigned_runner_id: null,
    is_active: true,
    metadata: {},
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    timeout: '10m',
    max_retries: null,
    dead_letter_enabled: null,
    dead_letter_retention: null,
    dead_letter_operator_hint: null,
    dead_letter_replay_max_age: null,
    tags: ['reports'],
    ...overrides,
  }
}

function makeSchedule(overrides: Partial<TriggerDefinition> = {}): TriggerDefinition {
  return {
    trigger_id: 'trg-1',
    job_key: 'nightly-report',
    cron_expression: '0 3 * * *',
    timezone: null,
    calendar: null,
    window: null,
    enabled: true,
    managed_by: 'api',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  }
}

function makeCalendar(overrides: Partial<CalendarDefinition> = {}): CalendarDefinition {
  return {
    calendar_id: 'cal-1',
    name: 'de-holidays',
    timezone: 'Europe/Berlin',
    rules: 'skip "2026-12-25"\nskip "2026-12-26"',
    managed_by: 'api',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  }
}

describe('renderDsl', () => {
  it('omits the tz line when the schedule has no timezone', () => {
    const dsl = renderDsl(makeJob(), [makeSchedule({ timezone: null })], [])
    expect(dsl).toContain('  schedule {')
    expect(dsl).toContain('    rule = "0 3 * * *"')
    expect(dsl).not.toContain('tz   =')
  })

  it('renders the tz line when the schedule has a timezone', () => {
    const dsl = renderDsl(makeJob(), [makeSchedule({ timezone: 'Europe/Berlin' })], [])
    expect(dsl).toContain('    tz   = "Europe/Berlin"')
  })

  it('renders a calendar block for a resolvable calendar reference', () => {
    const dsl = renderDsl(
      makeJob(),
      [makeSchedule({ calendar: 'de-holidays' })],
      [makeCalendar()],
    )
    expect(dsl).toContain('# rendered from the live job + first attached schedule + its calendar')
    expect(dsl).toContain('    calendar = "de-holidays"')
    expect(dsl).toContain('calendar "de-holidays" {')
    expect(dsl).toContain('  timezone "Europe/Berlin"')
    // Calendar rules are re-indented one level inside the block.
    expect(dsl).toContain('  skip "2026-12-25"')
    expect(dsl).toContain('  skip "2026-12-26"')
    expect(dsl).not.toContain('could not be resolved')
  })

  it('flags an unresolved calendar once the calendar list has loaded', () => {
    const dsl = renderDsl(
      makeJob(),
      [makeSchedule({ calendar: 'de-holidays' })],
      [makeCalendar({ name: 'other-calendar' })],
    )
    expect(dsl).toContain('# calendar "de-holidays" is referenced but could not be resolved')
    expect(dsl).not.toContain('calendar "de-holidays" {')
    expect(dsl).not.toContain('+ its calendar')
  })

  it('stays silent about the calendar while the calendar list is still loading', () => {
    const dsl = renderDsl(makeJob(), [makeSchedule({ calendar: 'de-holidays' })], undefined)
    expect(dsl).not.toContain('calendar "de-holidays" {')
    expect(dsl).not.toContain('could not be resolved')
    expect(dsl).not.toContain('+ its calendar')
  })

  it('omits the schedule block for a job without schedules', () => {
    const dsl = renderDsl(makeJob(), [], [])
    expect(dsl).not.toContain('schedule {')
    expect(dsl).toContain('job "nightly-report" {')
  })

  it('flags a disabled first schedule', () => {
    const dsl = renderDsl(makeJob(), [makeSchedule({ enabled: false })], [])
    expect(dsl).toContain('  # this schedule is currently disabled')
    expect(dsl).toContain('  schedule {')
  })

  it('surfaces extra API-attached schedules as comments', () => {
    const dsl = renderDsl(
      makeJob(),
      [
        makeSchedule(),
        makeSchedule({
          trigger_id: 'trg-2',
          cron_expression: '0 12 * * *',
          timezone: 'Europe/Berlin',
          calendar: 'de-holidays',
        }),
        makeSchedule({ trigger_id: 'trg-3', cron_expression: '0 18 * * *', enabled: false }),
      ],
      [],
    )
    expect(dsl).toContain('  # +2 more schedules attached via API (a job block holds a single schedule)')
    expect(dsl).toContain('  #   rule "0 12 * * *" · tz Europe/Berlin · calendar de-holidays')
    expect(dsl).toContain('  #   rule "0 18 * * *" · (disabled)')
    // Only the first trigger renders as a real schedule block.
    expect(dsl.match(/ {2}schedule \{/g)).toHaveLength(1)
  })

  it('does not add an extra-schedules comment for a single schedule', () => {
    const dsl = renderDsl(makeJob(), [makeSchedule()], [])
    expect(dsl).not.toContain('more schedule')
  })
})
