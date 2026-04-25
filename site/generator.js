// Croniqfile DSL generator — standalone, no auth, no framework.
//
// Loads the wasm-compiled croniq-config bridge from `./wasm/`, drives
// two form panels (Schedule and Calendar), and renders live DSL +
// preview output. The wasm bundle is gzipped ~70 KB; we lazy-load on
// first interaction so the page paints immediately.

import init, * as wasm from './wasm/croniq_config_wasm.js'

// ── Wasm loader ──────────────────────────────────────────────────────

let wasmReady = null
function ensureWasm() {
  if (!wasmReady) wasmReady = init()
  return wasmReady
}

// ── Tab switching ───────────────────────────────────────────────────

document.querySelectorAll('.tab-btn').forEach((btn) => {
  btn.addEventListener('click', () => {
    document.querySelectorAll('.tab-btn').forEach((b) => {
      const active = b === btn
      b.classList.toggle('active', active)
      b.setAttribute('aria-selected', String(active))
    })
    document.querySelectorAll('.tab-panel').forEach((p) => {
      p.classList.toggle('active', p.id === `tab-${btn.dataset.tab}`)
    })
  })
})

// ── Schedule panel ──────────────────────────────────────────────────

const WEEKDAYS = ['monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday', 'sunday']
const WEEKDAY_SHORT = { monday: 'Mon', tuesday: 'Tue', wednesday: 'Wed', thursday: 'Thu', friday: 'Fri', saturday: 'Sat', sunday: 'Sun' }
const ORDINALS = ['1st', '2nd', '3rd', '4th', '5th', '6th', '7th', '8th', '9th', '10th',
  '11th', '12th', '13th', '14th', '15th', '16th', '17th', '18th', '19th', '20th',
  '21st', '22nd', '23rd', '24th', '25th', '26th', '27th', '28th', '29th', '30th', '31st', 'last']

const schState = {
  mode: 'interval',
  interval: { count: 5, unit: 'minutes' },
  daily: { hour: 9, minute: 0 },
  weekdays: { days: ['monday', 'tuesday', 'wednesday', 'thursday', 'friday'], hour: 9, minute: 0 },
  monthly: { ordinals: ['1st'], hour: 3, minute: 0 },
  once: { at: '2026-12-31T23:00:00Z' },
}

// Render weekday + ordinal toggle buttons once.
const wdHost = document.getElementById('sch-wd-days')
WEEKDAYS.forEach((d) => {
  const b = document.createElement('button')
  b.type = 'button'
  b.className = 'day-toggle'
  b.textContent = WEEKDAY_SHORT[d]
  b.dataset.day = d
  if (schState.weekdays.days.includes(d)) b.classList.add('active')
  b.addEventListener('click', () => {
    const i = schState.weekdays.days.indexOf(d)
    if (i >= 0) schState.weekdays.days.splice(i, 1)
    else schState.weekdays.days.push(d)
    b.classList.toggle('active')
    refreshSchedule()
  })
  wdHost.appendChild(b)
})

const ordHost = document.getElementById('sch-mth-ords')
ORDINALS.forEach((o) => {
  const b = document.createElement('button')
  b.type = 'button'
  b.className = 'ord-toggle'
  b.textContent = o
  b.dataset.ord = o
  if (schState.monthly.ordinals.includes(o)) b.classList.add('active')
  b.addEventListener('click', () => {
    const i = schState.monthly.ordinals.indexOf(o)
    if (i >= 0) schState.monthly.ordinals.splice(i, 1)
    else schState.monthly.ordinals.push(o)
    b.classList.toggle('active')
    refreshSchedule()
  })
  ordHost.appendChild(b)
})

// Mode dropdown swaps fieldset visibility.
const schModeEl = document.getElementById('sch-mode')
schModeEl.addEventListener('change', () => {
  schState.mode = schModeEl.value
  document.querySelectorAll('.sch-fields').forEach((el) => {
    el.hidden = el.id !== `sch-fields-${schState.mode}`
  })
  refreshSchedule()
})

// Per-mode field bindings — tiny wrappers that mutate schState then
// re-render. Each input's `change` event triggers a refresh.
function bindNumber(id, getter, setter) {
  const el = document.getElementById(id)
  el.value = getter()
  el.addEventListener('input', () => {
    setter(parseInt(el.value, 10) || 0)
    refreshSchedule()
  })
}
function bindTime(id, getter, setter) {
  const el = document.getElementById(id)
  const { hour, minute } = getter()
  el.value = `${String(hour).padStart(2, '0')}:${String(minute).padStart(2, '0')}`
  el.addEventListener('input', () => {
    const [h, m] = (el.value || '0:0').split(':').map((s) => parseInt(s, 10) || 0)
    setter(h, m)
    refreshSchedule()
  })
}
function bindText(id, getter, setter) {
  const el = document.getElementById(id)
  el.value = getter()
  el.addEventListener('input', () => {
    setter(el.value)
    refreshSchedule()
  })
}
function bindSelect(id, getter, setter) {
  const el = document.getElementById(id)
  el.value = getter()
  el.addEventListener('change', () => {
    setter(el.value)
    refreshSchedule()
  })
}

bindNumber('sch-int-count', () => schState.interval.count, (v) => { schState.interval.count = v })
bindSelect('sch-int-unit', () => schState.interval.unit, (v) => { schState.interval.unit = v })
bindTime('sch-daily-time', () => schState.daily, (h, m) => { schState.daily.hour = h; schState.daily.minute = m })
bindTime('sch-wd-time', () => schState.weekdays, (h, m) => { schState.weekdays.hour = h; schState.weekdays.minute = m })
bindTime('sch-mth-time', () => schState.monthly, (h, m) => { schState.monthly.hour = h; schState.monthly.minute = m })
bindText('sch-once-at', () => schState.once.at, (v) => { schState.once.at = v })

function buildSchedulePayload() {
  const m = schState.mode
  if (m === 'interval') return { mode: 'interval', count: schState.interval.count, unit: schState.interval.unit }
  if (m === 'daily') return { mode: 'daily', hour: schState.daily.hour, minute: schState.daily.minute }
  if (m === 'weekdays') return { mode: 'weekdays', days: schState.weekdays.days.slice(), hour: schState.weekdays.hour, minute: schState.weekdays.minute }
  if (m === 'monthly') return { mode: 'monthly', ordinals: schState.monthly.ordinals.slice(), hour: schState.monthly.hour, minute: schState.monthly.minute }
  if (m === 'once') return { mode: 'once', at: schState.once.at }
  return { mode: 'disabled' }
}

const schDslEl = document.getElementById('sch-dsl')
const schErrEl = document.getElementById('sch-error')
const schFiresEl = document.getElementById('sch-fires')

async function refreshSchedule() {
  await ensureWasm()
  schErrEl.hidden = true
  let dsl = ''
  try {
    dsl = wasm.formatSchedule(buildSchedulePayload())
  } catch (e) {
    schDslEl.textContent = ''
    schErrEl.hidden = false
    schErrEl.textContent = String(e)
    schFiresEl.textContent = ''
    return
  }
  schDslEl.textContent = dsl

  // Live next-fires preview, current UTC instant. The wasm crate's
  // next-fire path is UTC-only by design (see PR #55) — for the
  // preview that's exactly what we want.
  const now = new Date().toISOString().replace(/\.\d{3}Z$/, 'Z')
  let result
  try {
    result = wasm.nextFires(dsl, now, 5)
  } catch (e) {
    result = { ok: false, fires: [], error: String(e) }
  }
  schFiresEl.innerHTML = ''
  if (!result.ok || result.fires.length === 0) {
    const li = document.createElement('li')
    li.textContent = result.error || (schState.mode === 'disabled' ? '(disabled)' : '(no upcoming fires)')
    li.style.color = 'var(--fg-muted)'
    schFiresEl.appendChild(li)
    return
  }
  result.fires.forEach((iso) => {
    const li = document.createElement('li')
    li.textContent = iso
    schFiresEl.appendChild(li)
  })
}

document.getElementById('sch-copy').addEventListener('click', async (e) => {
  await navigator.clipboard.writeText(schDslEl.textContent)
  const btn = e.currentTarget
  const orig = btn.textContent
  btn.textContent = 'Copied!'
  setTimeout(() => { btn.textContent = orig }, 1200)
})

// ── Calendar panel ──────────────────────────────────────────────────

const calState = {
  rules: [
    { action: 'include', rule_type: 'weekly', args: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri'] },
    { action: 'exclude', rule_type: 'annual', args: ['12-25'] },
  ],
  // Visible month — initialised to the current real-world month.
  view: { year: new Date().getUTCFullYear(), month: new Date().getUTCMonth() + 1 },
}

const calRulesEl = document.getElementById('cal-rules')
const calDslEl = document.getElementById('cal-dsl')
const calErrEl = document.getElementById('cal-error')
const calMonthLbl = document.getElementById('cal-month')
const calGridEl = document.getElementById('cal-grid')

const RULE_TYPES = ['weekly', 'window', 'monthly', 'annual', 'timezone']
const RULE_ARG_HINTS = {
  weekly: 'Days: e.g. Mon Tue Wed (space-separated 3-letter)',
  window: 'Window: e.g. 08:00..18:00 (single arg)',
  monthly: 'Days: e.g. 1 15 (space-separated, "last" allowed)',
  annual: 'Date: MM-DD (e.g. 12-25)',
  timezone: 'IANA name: e.g. Europe/Vienna',
}

function renderRuleEditor() {
  calRulesEl.innerHTML = ''
  calState.rules.forEach((rule, idx) => {
    const row = document.createElement('div')
    row.className = 'rule-row'

    const action = document.createElement('select')
    ;['include', 'exclude'].forEach((a) => {
      const opt = document.createElement('option')
      opt.value = a; opt.textContent = a
      action.appendChild(opt)
    })
    action.value = rule.action
    action.addEventListener('change', () => { rule.action = action.value; refreshCalendar() })

    const ruleType = document.createElement('select')
    RULE_TYPES.forEach((t) => {
      const opt = document.createElement('option')
      opt.value = t; opt.textContent = t
      ruleType.appendChild(opt)
    })
    ruleType.value = rule.rule_type
    ruleType.addEventListener('change', () => {
      rule.rule_type = ruleType.value
      // Reset args when the rule type changes — the args have type-
      // specific shape (single-string-with-dotdot vs. space-separated
      // tokens) and a stale carry-over would silently mis-render.
      rule.args = []
      argsInput.value = ''
      argsInput.placeholder = RULE_ARG_HINTS[ruleType.value] || ''
      refreshCalendar()
    })

    const argsInput = document.createElement('input')
    argsInput.type = 'text'
    argsInput.placeholder = RULE_ARG_HINTS[rule.rule_type] || ''
    argsInput.value = serializeArgs(rule)
    argsInput.addEventListener('input', () => {
      rule.args = parseArgs(rule.rule_type, argsInput.value)
      refreshCalendar()
    })

    const remove = document.createElement('button')
    remove.className = 'rule-remove'
    remove.type = 'button'
    remove.setAttribute('aria-label', `Remove rule ${idx + 1}`)
    remove.textContent = '×'
    remove.addEventListener('click', () => {
      calState.rules.splice(idx, 1)
      renderRuleEditor()
      refreshCalendar()
    })

    row.appendChild(action)
    row.appendChild(ruleType)
    row.appendChild(argsInput)
    row.appendChild(remove)
    calRulesEl.appendChild(row)
  })
}

// Args are stored as a list of strings that round-trip cleanly through
// the wasm format/parse pair. The UI exposes them as a single text
// box (it's a doc-page tool, not a full form) — these helpers split
// the text into the right shape per rule type.
function parseArgs(ruleType, text) {
  const t = (text || '').trim()
  if (!t) return []
  if (ruleType === 'window') {
    // The parser expects `"HH:MM".."HH:MM"` as a single argument
    // string. The caller types `08:00..18:00` and we split on `..`.
    const [a, b] = t.split('..').map((s) => s.replace(/^"|"$/g, '').trim())
    return a && b ? [a, b] : [t]
  }
  // weekly / monthly / annual / timezone — space-separated tokens.
  return t.split(/\s+/)
}
function serializeArgs(rule) {
  if (rule.rule_type === 'window' && rule.args.length === 2) {
    return `${rule.args[0]}..${rule.args[1]}`
  }
  return rule.args.join(' ')
}

document.getElementById('cal-add-rule').addEventListener('click', () => {
  calState.rules.push({ action: 'include', rule_type: 'weekly', args: [] })
  renderRuleEditor()
  refreshCalendar()
})

async function refreshCalendar() {
  await ensureWasm()
  calErrEl.hidden = true
  let dsl = ''
  try {
    dsl = wasm.formatCalendarRules(calState.rules)
  } catch (e) {
    calDslEl.textContent = ''
    calErrEl.hidden = false
    calErrEl.textContent = String(e)
    return
  }
  calDslEl.textContent = dsl

  // Validate by re-parsing. If the parser rejects, surface the error
  // and skip grid-rendering — old grid stays visible until the user
  // fixes the input.
  let parsed
  try { parsed = wasm.parseCalendarRules(dsl) } catch (e) { parsed = { ok: false, diagnostics: [String(e)] } }
  if (!parsed.ok) {
    calErrEl.hidden = false
    calErrEl.textContent = parsed.diagnostics.join('\n')
    return
  }
  renderCalendarGrid()
}

document.getElementById('cal-copy').addEventListener('click', async (e) => {
  await navigator.clipboard.writeText(calDslEl.textContent)
  const btn = e.currentTarget
  const orig = btn.textContent
  btn.textContent = 'Copied!'
  setTimeout(() => { btn.textContent = orig }, 1200)
})

document.getElementById('cal-prev').addEventListener('click', () => {
  if (calState.view.month === 1) {
    calState.view.month = 12
    calState.view.year -= 1
  } else {
    calState.view.month -= 1
  }
  renderCalendarGrid()
})
document.getElementById('cal-next').addEventListener('click', () => {
  if (calState.view.month === 12) {
    calState.view.month = 1
    calState.view.year += 1
  } else {
    calState.view.month += 1
  }
  renderCalendarGrid()
})

// ── Calendar evaluation (UTC, statically — full timezone-aware
// evaluation lives in the scheduler; the preview here demonstrates
// rule effect, not a precise day-trigger schedule).

function evaluateDay(date) {
  // Last-rule-wins per-day evaluation. `include` adds the day to the
  // active set, `exclude` removes it. Empty rule list ⇒ no rule fires
  // (matches the scheduler's "no rules ⇒ everything excluded" only
  // for the *include-then-exclude* flow; here we render uncovered
  // days as neutral so the grid distinguishes "not covered" from
  // "explicitly excluded").
  let state = 'none'
  for (const rule of calState.rules) {
    if (!ruleMatches(rule, date)) continue
    state = rule.action === 'include' ? 'included' : 'excluded'
  }
  return state
}

const SHORT_DAY = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']

function ruleMatches(rule, date) {
  if (rule.rule_type === 'weekly') {
    return rule.args.some((a) => a.replace(/"/g, '').toLowerCase() === SHORT_DAY[date.getUTCDay()].toLowerCase())
  }
  if (rule.rule_type === 'monthly') {
    return rule.args.some((a) => {
      if (a === 'last') {
        const last = new Date(Date.UTC(date.getUTCFullYear(), date.getUTCMonth() + 1, 0)).getUTCDate()
        return date.getUTCDate() === last
      }
      return parseInt(a, 10) === date.getUTCDate()
    })
  }
  if (rule.rule_type === 'annual') {
    return rule.args.some((a) => {
      const m = a.match(/^(\d{1,2})-(\d{1,2})$/)
      if (!m) return false
      return date.getUTCMonth() + 1 === parseInt(m[1], 10) && date.getUTCDate() === parseInt(m[2], 10)
    })
  }
  if (rule.rule_type === 'window') {
    // Window is time-of-day, not a per-day predicate. For the day-grid
    // we treat window rules as "no effect on whether the day is
    // active" — the actual scheduler evaluates them inside the day.
    // Render neutrally (don't match) so the grid stays informative
    // without lying about hour-of-day.
    return false
  }
  return false
}

function renderCalendarGrid() {
  const { year, month } = calState.view
  calMonthLbl.textContent = `${new Date(Date.UTC(year, month - 1, 1)).toLocaleString('en-US', { month: 'long', year: 'numeric', timeZone: 'UTC' })}`
  calGridEl.innerHTML = ''

  // Headers Mon-first (matches the scheduler's weekday set).
  ;['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'].forEach((d) => {
    const h = document.createElement('div')
    h.className = 'cal-head'
    h.textContent = d
    calGridEl.appendChild(h)
  })

  // First Monday on or before the 1st of the visible month.
  const first = new Date(Date.UTC(year, month - 1, 1))
  const firstDay = (first.getUTCDay() + 6) % 7 // 0 = Mon
  const start = new Date(first)
  start.setUTCDate(1 - firstDay)

  const todayIso = new Date().toISOString().slice(0, 10)
  for (let i = 0; i < 42; i++) {
    const d = new Date(start)
    d.setUTCDate(start.getUTCDate() + i)
    const cell = document.createElement('div')
    cell.className = 'cal-day'
    cell.textContent = String(d.getUTCDate())
    if (d.getUTCMonth() + 1 !== month) cell.classList.add('outside')
    if (d.toISOString().slice(0, 10) === todayIso) cell.classList.add('today')
    const state = evaluateDay(d)
    if (state === 'included') cell.classList.add('included')
    if (state === 'excluded') cell.classList.add('excluded')
    calGridEl.appendChild(cell)
  }
}

// ── Bootstrap ──────────────────────────────────────────────────────

renderRuleEditor()
refreshSchedule()
refreshCalendar()
