import type { CalendarDefinition, JobDefinition, TriggerDefinition } from '@/api/types'

/// Renders a job (plus its first attached schedule and, if resolvable,
/// the referenced calendar) as Croniqfile DSL text for the read-only
/// "DSL" tab on the job detail page. Additional API-registered triggers
/// can't be expressed in the single schedule block a job holds, so they
/// are surfaced as comments.
export function renderDsl(
  job: JobDefinition,
  schedules: TriggerDefinition[],
  calendars: CalendarDefinition[] | undefined,
): string {
  const tags = JSON.stringify(job.tags ?? [])
  const timeout = job.timeout ?? '5m'
  const sched = schedules[0]
  // The grammar allows at most one schedule block per job block, so jobs with
  // additional API-registered triggers get them surfaced as comments instead.
  const extraSchedules = schedules.slice(1)
  const calendar = sched?.calendar ? calendars?.find((c) => c.name === sched.calendar) : undefined
  // Only flag an unresolved reference once the calendar list has loaded.
  const calendarMissing = Boolean(sched?.calendar && calendars && !calendar)
  return [
    `# ${job.job_key}`,
    `# rendered from the live job + first attached schedule${calendar ? ' + its calendar' : ''}`,
    ``,
    `job "${job.job_key}" {`,
    `  description = ${JSON.stringify(job.description ?? '')}`,
    `  tags        = ${tags}`,
    `  timeout     = "${timeout}"`,
    ...(job.max_retries != null ? [`  max_retries = ${job.max_retries}`] : []),
    ...(job.dead_letter_enabled === false ? [`  dead_letter { enabled = false }`] : []),
    ...(sched
      ? [
          ``,
          ...(sched.enabled === false ? [`  # this schedule is currently disabled`] : []),
          `  schedule {`,
          `    rule = ${JSON.stringify(sched.cron_expression ?? '')}`,
          ...(sched.timezone ? [`    tz   = "${sched.timezone}"`] : []),
          ...(sched.calendar ? [`    calendar = "${sched.calendar}"`] : []),
          ...(sched.window ? [`    window   = "${sched.window}"`] : []),
          `  }`,
        ]
      : []),
    ...(extraSchedules.length > 0
      ? [
          ``,
          `  # +${extraSchedules.length} more schedule${extraSchedules.length === 1 ? '' : 's'} attached via API (a job block holds a single schedule)`,
          ...extraSchedules.map((t) => `  #   ${describeTriggerShort(t)}`),
        ]
      : []),
    `}`,
    ...(calendar
      ? [
          ``,
          `calendar "${calendar.name}" {`,
          ...(calendar.timezone ? [`  timezone "${calendar.timezone}"`] : []),
          ...calendar.rules
            .split('\n')
            .map((line) => line.trim())
            .filter(Boolean)
            .map((line) => `  ${line}`),
          `}`,
        ]
      : []),
    ...(calendarMissing
      ? [``, `# calendar "${sched.calendar}" is referenced but could not be resolved`]
      : []),
    ``,
  ].join('\n')
}

/// One-line summary of a trigger that can't be expressed as a schedule block
/// (the job block already holds one) — rendered as a DSL comment.
function describeTriggerShort(t: TriggerDefinition): string {
  const parts = [`rule ${JSON.stringify(t.cron_expression ?? '')}`]
  if (t.timezone) parts.push(`tz ${t.timezone}`)
  if (t.calendar) parts.push(`calendar ${t.calendar}`)
  if (t.window) parts.push(`window ${t.window}`)
  if (t.enabled === false) parts.push('(disabled)')
  return parts.join(' · ')
}
