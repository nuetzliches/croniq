// The croniq-config-wasm bridge, typed.
//
// Ported from the React tree's `lib/croniq-dsl.ts`. The compiler that parses
// and formats Croniqfile fragments is the *same* Rust crate the server uses,
// compiled to wasm — which is the whole point: a rule builder that agreed with
// a hand-written parser right up until it didn't would be worse than no
// builder at all.
//
// Loaded lazily on first call, so a session that never opens a calendar or
// schedule editor does not pay for the binary.

// The .js file is a wasm-bindgen loader and the .wasm sits beside it, fetched
// relative to the loader URL. Vite copies both into the bundle as
// fingerprinted assets; only the loader is imported here.
import init, * as wasm from './wasm/croniq_config_wasm.js'

export type ScheduleMode = 'interval' | 'daily' | 'weekdays' | 'monthly' | 'once' | 'disabled'

export type SchedulePayload =
  | { mode: 'interval'; count: number; unit: 'seconds' | 'minutes' | 'hours' }
  | { mode: 'daily'; hour: number; minute: number }
  | { mode: 'weekdays'; days: string[]; hour: number; minute: number }
  | { mode: 'monthly'; ordinals: string[]; hour: number; minute: number }
  | { mode: 'once'; at: string }
  | { mode: 'disabled' }

export interface ParseScheduleResult {
  ok: boolean
  schedule: SchedulePayload | null
  error: string | null
}

export interface NextFiresResult {
  ok: boolean
  fires: string[]
  error: string | null
}

export interface CalendarRulePayload {
  action: 'include' | 'exclude'
  rule_type: string
  args: string[]
}

export interface ParseCalendarResult {
  ok: boolean
  rules: CalendarRulePayload[]
  diagnostics: string[]
}

let initPromise: Promise<void> | null = null

/** One loader run, however many callers race for it. */
function ensureLoaded(): Promise<void> {
  initPromise ??= init().then(() => undefined)
  return initPromise
}

export async function parseSchedule(dsl: string): Promise<ParseScheduleResult> {
  await ensureLoaded()
  return wasm.parseSchedule(dsl) as ParseScheduleResult
}

export async function formatSchedule(payload: SchedulePayload): Promise<string> {
  await ensureLoaded()
  return wasm.formatSchedule(payload) as string
}

/**
 * The next `count` fire times for a schedule rule.
 *
 * Calendar-unaware: it answers what the *rule* says, not what the calendar
 * gating it would allow. Nothing on the client can answer the latter — see
 * the note in CalendarDetail.vue.
 */
export async function nextFires(
  dsl: string,
  nowIso: string,
  count: number,
): Promise<NextFiresResult> {
  await ensureLoaded()
  return wasm.nextFires(dsl, nowIso, count) as NextFiresResult
}

export async function parseCalendarRules(dsl: string): Promise<ParseCalendarResult> {
  await ensureLoaded()
  return wasm.parseCalendarRules(dsl) as ParseCalendarResult
}

export async function formatCalendarRules(rules: CalendarRulePayload[]): Promise<string> {
  await ensureLoaded()
  return wasm.formatCalendarRules(rules) as string
}
