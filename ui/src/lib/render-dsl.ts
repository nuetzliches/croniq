import type { CalendarDefinition, JobDefinition, TriggerDefinition } from '@/api/types'

/// Renders a job (plus its first attached schedule and, if resolvable,
/// the referenced calendar) as Croniqfile DSL text for the read-only
/// "DSL" tab on the job detail page.
export function renderDsl(
  job: JobDefinition,
  schedules: TriggerDefinition[],
  calendars: CalendarDefinition[] | undefined,
): string {
  const tags = JSON.stringify(job.tags ?? [])
  const timeout = job.timeout ?? '5m'
  const sched = schedules[0]
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
          `  schedule {`,
          `    rule = ${JSON.stringify(sched.cron_expression ?? '')}`,
          ...(sched.timezone ? [`    tz   = "${sched.timezone}"`] : []),
          ...(sched.calendar ? [`    calendar = "${sched.calendar}"`] : []),
          ...(sched.window ? [`    window   = "${sched.window}"`] : []),
          `  }`,
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
