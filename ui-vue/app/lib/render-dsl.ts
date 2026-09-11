import type { CalendarDefinition, JobDefinition, TriggerDefinition } from '~/api/types'
import {
  formatCalendarBlock,
  formatJobBlock,
  parseCalendarRules,
  parseSchedule,
  type SchedulePayload,
} from '~/lib/croniq-dsl'

/**
 * A job, rendered back as Croniqfile text that actually parses.
 *
 * The first version of this file was ported verbatim from the React tree and
 * emitted something that looked like the DSL and was not:
 *
 *     job "demo:report" {
 *       description = "Generates a summary report"
 *       tags        = ["env=demo"]
 *     }
 *
 * The real grammar has no `=`, does not quote the job key, and writes tags as
 * a bare list. Run through croniq's own lexer, that text fails on line 2. The
 * tab presented it as "the job as it is written", and a reader who pasted it
 * into a Croniqfile got a parse error for their trouble.
 *
 * So none of it is assembled by hand any more. `formatJobBlock` and
 * `formatCalendarBlock` come from `croniq-config` compiled to wasm — the same
 * crate the server loads a Croniqfile with — and the Rust side parses its own
 * output before returning it, then re-emits it through the canonical
 * formatter. Whatever comes back parses, by construction. That is the same
 * argument the calendar rule builder is built on, and this file is what
 * happens when it is not applied.
 *
 * What the API cannot tell us is reported rather than guessed at: see `notes`.
 */
export interface RenderedDsl {
  /** The block, or `''` when the job could not be expressed. */
  text: string
  /**
   * What the rendering could not carry across, in the reader's words. Empty
   * when the text is a complete account of the job.
   */
  notes: string[]
}

export async function renderJobDsl(
  job: JobDefinition,
  schedules: TriggerDefinition[],
  calendars: CalendarDefinition[] | undefined,
): Promise<RenderedDsl> {
  const notes: string[] = []
  const trigger = schedules[0]

  // A job block holds one schedule; further API-registered triggers have no
  // spelling here at all. Saying so beats rendering a job that quietly runs
  // more often than the text claims.
  if (schedules.length > 1) {
    notes.push(
      `${schedules.length - 1} further schedule${schedules.length === 2 ? '' : 's'} ${
        schedules.length === 2 ? 'is' : 'are'
      } attached through the API. A job block holds one, so ${
        schedules.length === 2 ? 'it is' : 'they are'
      } not in the text below.`,
    )
  }

  const payload = await schedulePayload(trigger, notes)
  if (!payload) return { text: '', notes }

  // The stored job carries a retry *count* and no strategy. Emitting one would
  // put a word in the file that nothing in the API said — so the block leaves
  // retry to `defaults { }` and the note says why.
  if (job.max_retries != null) {
    notes.push(
      `The job retries ${job.max_retries} time${job.max_retries === 1 ? '' : 's'}, but the API does not record which backoff strategy, so no retry block is written — it would have to invent one. Add it yourself, or let defaults { } supply it.`,
    )
  }

  const options = {
    description: job.description || undefined,
    timeout: job.timeout || undefined,
    tags: job.tags ?? [],
    dead_letter: deadLetterOptions(job),
    schedule_calendar: trigger?.calendar || undefined,
    schedule_timezone: trigger?.timezone || undefined,
  }

  let text: string
  try {
    text = await formatJobBlock(payload, job.job_key, options)
  } catch (caught) {
    // The formatter rejects what the parser would reject — a malformed key, an
    // unparseable duration. Its message is the useful part.
    notes.push(`This job cannot be expressed in the DSL: ${asMessage(caught)}`)
    return { text: '', notes }
  }

  const calendarBlock = await renderCalendar(trigger, calendars, notes)
  return { text: calendarBlock ? `${text}\n${calendarBlock}` : text, notes }
}

/**
 * The stored rule, as a structured schedule.
 *
 * A trigger holds whatever string the API accepted, and the API accepts cron
 * expressions the Croniqfile grammar has no form for. That is not a rendering
 * bug to work around — such a job genuinely cannot be written down as DSL, and
 * the honest answer is to say which rule it was.
 */
async function schedulePayload(
  trigger: TriggerDefinition | undefined,
  notes: string[],
): Promise<SchedulePayload | null> {
  const rule = trigger?.cron_expression?.trim()
  // No trigger is not a gap: such a job only runs when something fires it, and
  // `disabled` is how the DSL spells exactly that.
  if (!rule) return { mode: 'disabled' }

  const parsed = await parseSchedule(rule)
  if (parsed.ok && parsed.schedule) return parsed.schedule

  notes.push(
    `The schedule "${rule}" has no Croniqfile spelling${parsed.error ? ` (${parsed.error})` : ''}. The DSL expresses intervals, daily, weekday and monthly schedules — not raw cron — so this job cannot be written down as it currently runs.`,
  )
  return null
}

function deadLetterOptions(job: JobDefinition) {
  const block = {
    enabled: job.dead_letter_enabled ?? undefined,
    retention: job.dead_letter_retention || undefined,
    operator_hint: job.dead_letter_operator_hint || undefined,
    replay_max_age: job.dead_letter_replay_max_age || undefined,
  }
  return Object.values(block).some((value) => value !== undefined) ? block : undefined
}

/** The referenced calendar, inlined — so the rendered text stands on its own. */
async function renderCalendar(
  trigger: TriggerDefinition | undefined,
  calendars: CalendarDefinition[] | undefined,
  notes: string[],
): Promise<string | null> {
  const name = trigger?.calendar
  if (!name) return null
  // `undefined` means the list has not loaded, which is not the same as the
  // calendar not being there — do not accuse on missing information.
  if (!calendars) return null

  const calendar = calendars.find((candidate) => candidate.name === name)
  if (!calendar) {
    notes.push(
      `The schedule references a calendar named "${name}" and no such calendar exists. A Croniqfile with this job would fail to load.`,
    )
    return null
  }

  const parsed = await parseCalendarRules(calendar.rules)
  if (!parsed.ok) {
    notes.push(
      `The calendar "${name}" has rules that did not parse${parsed.diagnostics.length ? `: ${parsed.diagnostics.join('; ')}` : ''}, so its block is not included.`,
    )
    return null
  }

  try {
    return await formatCalendarBlock(parsed.rules, calendar.name)
  } catch (caught) {
    notes.push(`The calendar "${name}" could not be rendered: ${asMessage(caught)}`)
    return null
  }
}

function asMessage(caught: unknown): string {
  if (caught instanceof Error) return caught.message
  if (typeof caught === 'string') return caught
  return String(caught)
}
