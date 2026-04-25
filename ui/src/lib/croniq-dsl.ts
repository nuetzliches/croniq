/// Typed wrapper around the croniq-config-wasm bridge.
///
/// The wasm module is loaded lazily on first call so that pages which
/// never open the schedule/calendar dialogs don't pay for the ~70 KB
/// gzipped download. Once loaded, the same instance is reused for the
/// lifetime of the page.

// The .js file is a wasm-bindgen loader; the .wasm sits next to it and
// is fetched relative to the loader URL. Vite copies both into the
// final bundle as fingerprinted assets — we only need the loader.
//
// `vite-ignore` is necessary because the loader is generated, not part
// of the typed module graph; without it Vite's import-analysis would
// complain about the relative .wasm path inside the loader.
import init, * as wasm from './wasm/croniq_config_wasm.js'

export type ScheduleMode =
  | 'interval'
  | 'daily'
  | 'weekdays'
  | 'monthly'
  | 'once'
  | 'disabled'

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

/// Fire the wasm-bindgen loader exactly once. Subsequent calls return
/// the same promise — even on parallel invocations during the first
/// render (StrictMode double-mount, Suspense, etc.) we don't double-
/// fetch the binary.
function ensureLoaded(): Promise<void> {
  if (!initPromise) {
    initPromise = init().then(() => undefined)
  }
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
