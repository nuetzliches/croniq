import type { CalendarDefinition, JobDefinition, TriggerDefinition } from '~/api/types'

/**
 * A job, rendered back as Croniqfile text.
 *
 * Ported from the React tree's `lib/render-dsl.ts`, unchanged in what it
 * emits. It stays a pure function of three arrays so it can be tested without
 * a server, which matters more here than usual: the output is something an
 * operator may paste into a Croniqfile, so a wrong quote or a dropped field is
 * not a cosmetic bug.
 *
 * Why render at all rather than ask the server: there is no endpoint that
 * gives the DSL for one job. The Croniqfile is the input, the API store is the
 * result, and for an API-managed job no source text exists — it was never
 * written down. So this reconstructs it, and says so in the header comment.
 */
export function renderDsl(
  job: JobDefinition,
  schedules: TriggerDefinition[],
  calendars: CalendarDefinition[] | undefined,
): string {
  const tags = JSON.stringify(job.tags ?? [])
  const timeout = job.timeout ?? '5m'
  const sched = schedules[0]
  // The grammar allows at most one schedule block per job, so any further
  // API-registered triggers are surfaced as comments rather than silently
  // dropped — the rendered text would otherwise claim less than is running.
  const extraSchedules = schedules.slice(1)
  const calendar = sched?.calendar ? calendars?.find((c) => c.name === sched.calendar) : undefined
  // Only call a reference unresolved once the calendar list has actually
  // loaded; `undefined` means "not known yet", not "not there".
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
          ...extraSchedules.map((trigger) => `  #   ${describeTriggerShort(trigger)}`),
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

/** One line for a trigger that has no room in the job block, as a comment. */
function describeTriggerShort(trigger: TriggerDefinition): string {
  const parts = [`rule ${JSON.stringify(trigger.cron_expression ?? '')}`]
  if (trigger.timezone) parts.push(`tz ${trigger.timezone}`)
  if (trigger.calendar) parts.push(`calendar ${trigger.calendar}`)
  if (trigger.window) parts.push(`window ${trigger.window}`)
  if (trigger.enabled === false) parts.push('(disabled)')
  return parts.join(' · ')
}
