// Croniqfile DSL generator — standalone, no auth, no framework.
//
// Loads the wasm-compiled croniq-config bridge from `./wasm/`, drives
// two form panels (Schedule and Calendar), and renders live DSL +
// preview output. The wasm bundle is gzipped ~70 KB; we lazy-load on
// first interaction so the page paints immediately.

// Cache-bust both the JS shim and the .wasm binary on every release.
// Bump WASM_VERSION whenever `site/wasm/` is rebuilt — otherwise long-
// lived browser/CDN caches will keep serving an old bundle and the DSL
// output drifts from the actual config crate.
const WASM_VERSION = '2026-04-26b'

import init, * as wasm from './wasm/croniq_config_wasm.js?v=2026-04-26b'

// ── Wasm loader ──────────────────────────────────────────────────────

let wasmReady = null
function ensureWasm() {
  if (!wasmReady) {
    wasmReady = init(new URL(`./wasm/croniq_config_wasm_bg.wasm?v=${WASM_VERSION}`, import.meta.url))
  }
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

const RULE_TYPE_LABELS = {
  weekly: 'Weekdays',
  window: 'Time window',
  monthly: 'Days of month',
  annual: 'Specific date',
  timezone: 'Timezone',
}

const SHORT_DAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']
const MONTHLY_ORDINALS = [
  '1', '2', '3', '4', '5', '6', '7', '8', '9', '10',
  '11', '12', '13', '14', '15', '16', '17', '18', '19', '20',
  '21', '22', '23', '24', '25', '26', '27', '28', '29', '30',
  '31', 'last',
]
const MONTH_LABELS = [
  'Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun',
  'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec',
]

// IANA list — populated lazily for the timezone rule's <datalist>.
// `Intl.supportedValuesOf` is in all evergreen browsers; we still
// gracefully degrade to a free-form text field if it isn't.
const IANA_TIMEZONES = (() => {
  try {
    return Intl.supportedValuesOf('timeZone')
  } catch {
    return []
  }
})()

function renderRuleEditor() {
  calRulesEl.innerHTML = ''
  calState.rules.forEach((rule, idx) => {
    const row = document.createElement('div')
    row.className = 'rule-row'

    const head = document.createElement('div')
    head.className = 'rule-row-head'

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
      opt.value = t
      opt.textContent = `${RULE_TYPE_LABELS[t]} (${t})`
      ruleType.appendChild(opt)
    })
    ruleType.value = rule.rule_type
    ruleType.addEventListener('change', () => {
      rule.rule_type = ruleType.value
      // Reset args whenever the rule type changes — the args have
      // type-specific shape and a stale carry-over would silently
      // mis-render the DSL.
      rule.args = []
      renderRuleEditor()
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

    head.appendChild(action)
    head.appendChild(ruleType)
    head.appendChild(remove)
    row.appendChild(head)

    // Per-type structured editor. Each branch mutates `rule.args` in
    // place and calls `refreshCalendar()` so the live preview updates
    // immediately. The args shape stays compatible with the existing
    // wasm format/parse pair — only the UI changes, not the data.
    const body = document.createElement('div')
    body.className = 'rule-row-body'
    if (rule.rule_type === 'weekly') {
      body.appendChild(buildWeeklyEditor(rule))
    } else if (rule.rule_type === 'window') {
      body.appendChild(buildWindowEditor(rule))
    } else if (rule.rule_type === 'monthly') {
      body.appendChild(buildMonthlyEditor(rule))
    } else if (rule.rule_type === 'annual') {
      body.appendChild(buildAnnualEditor(rule))
    } else if (rule.rule_type === 'timezone') {
      body.appendChild(buildTimezoneEditor(rule))
    }
    row.appendChild(body)

    calRulesEl.appendChild(row)
  })
}

function buildWeeklyEditor(rule) {
  // Stored args may be 3-letter (`Mon`) or full (`monday`); normalise
  // to capitalised 3-letter for both display and storage so the WASM
  // formatter sees a consistent shape it can collapse to `weekday` /
  // `Mon..Fri` / etc.
  rule.args = rule.args.map(normaliseDay).filter((d) => d !== null)
  const wrap = document.createElement('div')
  wrap.className = 'rule-weekly'
  const grid = document.createElement('div')
  grid.className = 'day-grid'
  SHORT_DAYS.forEach((d) => {
    const b = document.createElement('button')
    b.type = 'button'
    b.className = 'day-toggle'
    b.textContent = d
    b.setAttribute('aria-pressed', String(rule.args.includes(d)))
    if (rule.args.includes(d)) b.classList.add('active')
    b.addEventListener('click', () => {
      const i = rule.args.indexOf(d)
      if (i >= 0) rule.args.splice(i, 1)
      else rule.args.push(d)
      // Keep stored order canonical so the formatter's range collapse
      // sees `Mon Tue Wed` rather than the click order.
      rule.args.sort((a, b2) => SHORT_DAYS.indexOf(a) - SHORT_DAYS.indexOf(b2))
      b.classList.toggle('active')
      b.setAttribute('aria-pressed', String(rule.args.includes(d)))
      refreshCalendar()
    })
    grid.appendChild(b)
  })
  wrap.appendChild(grid)

  const presets = document.createElement('div')
  presets.className = 'rule-presets'
  const presetEntries = [
    { label: 'Weekday', days: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri'] },
    { label: 'Weekend', days: ['Sat', 'Sun'] },
    { label: 'Every day', days: SHORT_DAYS.slice() },
  ]
  presetEntries.forEach((p) => {
    const b = document.createElement('button')
    b.type = 'button'
    b.className = 'rule-preset'
    b.textContent = p.label
    b.addEventListener('click', () => {
      rule.args = p.days.slice()
      renderRuleEditor()
      refreshCalendar()
    })
    presets.appendChild(b)
  })
  wrap.appendChild(presets)
  return wrap
}

function buildWindowEditor(rule) {
  const wrap = document.createElement('div')
  wrap.className = 'rule-window'
  const from = document.createElement('input')
  from.type = 'time'
  from.value = rule.args[0] ?? ''
  from.setAttribute('aria-label', 'Window start (UTC)')
  const to = document.createElement('input')
  to.type = 'time'
  to.value = rule.args[1] ?? ''
  to.setAttribute('aria-label', 'Window end (UTC)')
  function sync() {
    const a = from.value
    const b = to.value
    rule.args = a && b ? [a, b] : []
    refreshCalendar()
  }
  from.addEventListener('input', sync)
  to.addEventListener('input', sync)
  const sep = document.createElement('span')
  sep.className = 'rule-sep'
  sep.textContent = 'to'
  wrap.appendChild(from)
  wrap.appendChild(sep)
  wrap.appendChild(to)
  return wrap
}

function buildMonthlyEditor(rule) {
  // Stored as a list of "1".."31" + "last". Tolerate older "1st"-style
  // tokens by stripping the suffix.
  rule.args = rule.args.map((a) => a.replace(/^(\d+)(st|nd|rd|th)$/i, '$1').toLowerCase())
  const wrap = document.createElement('div')
  wrap.className = 'rule-monthly'
  const grid = document.createElement('div')
  grid.className = 'ord-grid'
  MONTHLY_ORDINALS.forEach((o) => {
    const b = document.createElement('button')
    b.type = 'button'
    b.className = 'ord-toggle'
    b.textContent = o
    b.setAttribute('aria-pressed', String(rule.args.includes(o)))
    if (rule.args.includes(o)) b.classList.add('active')
    b.addEventListener('click', () => {
      const i = rule.args.indexOf(o)
      if (i >= 0) rule.args.splice(i, 1)
      else rule.args.push(o)
      rule.args.sort((a, b2) => {
        if (a === 'last') return 1
        if (b2 === 'last') return -1
        return parseInt(a, 10) - parseInt(b2, 10)
      })
      b.classList.toggle('active')
      b.setAttribute('aria-pressed', String(rule.args.includes(o)))
      refreshCalendar()
    })
    grid.appendChild(b)
  })
  wrap.appendChild(grid)

  const presets = document.createElement('div')
  presets.className = 'rule-presets'
  const presetEntries = [
    { label: '1st', days: ['1'] },
    { label: '15th', days: ['15'] },
    { label: '1st + 15th', days: ['1', '15'] },
    { label: 'Last day', days: ['last'] },
  ]
  presetEntries.forEach((p) => {
    const b = document.createElement('button')
    b.type = 'button'
    b.className = 'rule-preset'
    b.textContent = p.label
    b.addEventListener('click', () => {
      rule.args = p.days.slice()
      renderRuleEditor()
      refreshCalendar()
    })
    presets.appendChild(b)
  })
  wrap.appendChild(presets)
  return wrap
}

function buildAnnualEditor(rule) {
  // Stored as ["MM-DD"]. Split into separate month/day controls so
  // the user gets a labelled month dropdown + a numeric day input
  // instead of a free-form `12-25` text field.
  const cur = rule.args[0] ?? ''
  const m = /^(\d{1,2})-(\d{1,2})$/.exec(cur)
  let month = m ? parseInt(m[1], 10) : 0
  let day = m ? parseInt(m[2], 10) : 0

  const wrap = document.createElement('div')
  wrap.className = 'rule-annual'

  const monthSel = document.createElement('select')
  monthSel.setAttribute('aria-label', 'Month')
  const blank = document.createElement('option')
  blank.value = '0'
  blank.textContent = 'Month…'
  monthSel.appendChild(blank)
  MONTH_LABELS.forEach((label, i) => {
    const opt = document.createElement('option')
    opt.value = String(i + 1)
    opt.textContent = label
    monthSel.appendChild(opt)
  })
  monthSel.value = String(month)

  const dayInp = document.createElement('input')
  dayInp.type = 'number'
  dayInp.min = '1'
  dayInp.max = '31'
  dayInp.placeholder = 'Day'
  dayInp.setAttribute('aria-label', 'Day of month')
  dayInp.value = day ? String(day) : ''

  const preview = document.createElement('span')
  preview.className = 'rule-preview'

  function sync() {
    if (!month || !day) {
      rule.args = []
      preview.textContent = ''
    } else {
      const mm = String(month).padStart(2, '0')
      const dd = String(day).padStart(2, '0')
      rule.args = [`${mm}-${dd}`]
      preview.textContent = `${MONTH_LABELS[month - 1]} ${day}`
    }
    refreshCalendar()
  }
  monthSel.addEventListener('change', () => { month = parseInt(monthSel.value, 10); sync() })
  dayInp.addEventListener('input', () => { day = parseInt(dayInp.value, 10) || 0; sync() })

  // Initial render of the preview label without firing refreshCalendar
  // (the row was just rebuilt by renderRuleEditor → refreshCalendar
  // already follows).
  if (month && day) preview.textContent = `${MONTH_LABELS[month - 1]} ${day}`

  wrap.appendChild(monthSel)
  wrap.appendChild(dayInp)
  wrap.appendChild(preview)
  return wrap
}

function buildTimezoneEditor(rule) {
  const wrap = document.createElement('div')
  wrap.className = 'rule-timezone'
  const inp = document.createElement('input')
  inp.type = 'text'
  inp.value = rule.args[0] ?? ''
  inp.placeholder = 'IANA name (type to search)'
  inp.setAttribute('aria-label', 'Timezone')
  if (IANA_TIMEZONES.length > 0) {
    // Lazy-create the shared <datalist>; reuse it across rules so we
    // don't ship a 300-entry DOM tree per timezone rule.
    const listId = 'cal-iana-tz-list'
    if (!document.getElementById(listId)) {
      const list = document.createElement('datalist')
      list.id = listId
      IANA_TIMEZONES.forEach((tz) => {
        const o = document.createElement('option')
        o.value = tz
        list.appendChild(o)
      })
      document.body.appendChild(list)
    }
    inp.setAttribute('list', listId)
  }
  inp.addEventListener('input', () => {
    const v = inp.value.trim()
    rule.args = v ? [v] : []
    refreshCalendar()
  })
  wrap.appendChild(inp)
  if (!rule.args[0]) {
    const hint = document.createElement('p')
    hint.className = 'rule-hint'
    let detected = ''
    try { detected = Intl.DateTimeFormat().resolvedOptions().timeZone } catch { /* noop */ }
    hint.textContent = detected ? `Detected: ${detected}` : ''
    if (detected) wrap.appendChild(hint)
  }
  return wrap
}

/// Normalise a weekday token to its capitalised 3-letter form
/// (`Mon`, `Tue`, ..., `Sun`). Returns `null` if the input doesn't
/// look like a weekday token — used to drop garbage when storage
/// transitions from the old free-text editor to the new picker.
function normaliseDay(s) {
  const lower = String(s).toLowerCase().slice(0, 3)
  switch (lower) {
    case 'mon': return 'Mon'
    case 'tue': return 'Tue'
    case 'wed': return 'Wed'
    case 'thu': return 'Thu'
    case 'fri': return 'Fri'
    case 'sat': return 'Sat'
    case 'sun': return 'Sun'
    default: return null
  }
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
const WEEKDAY_INDEX = { mon: 0, tue: 1, wed: 2, thu: 3, fri: 4, sat: 5, sun: 6 }
const WEEKDAY_ALIASES = {
  weekday: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri'],
  weekdays: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri'],
  weekend: ['Sat', 'Sun'],
}

/// Expand a single weekly arg into the list of 3-letter weekdays it
/// represents. Handles single tokens (`Mon`), aliases (`weekday`) and
/// `Mon..Fri` ranges. Range wrap-around (`Fri..Mon`) is supported so
/// users typing it in the Advanced (raw) tab see the right Active
/// Days highlight.
function expandWeeklyArg(arg) {
  const raw = String(arg).replace(/"/g, '').trim().toLowerCase()
  if (!raw) return []
  if (WEEKDAY_ALIASES[raw]) return WEEKDAY_ALIASES[raw].slice()
  const m = /^([a-z]{3,9})\.\.([a-z]{3,9})$/.exec(raw)
  if (m) {
    const a = m[1].slice(0, 3)
    const b = m[2].slice(0, 3)
    if (a in WEEKDAY_INDEX && b in WEEKDAY_INDEX) {
      const start = WEEKDAY_INDEX[a]
      const end = WEEKDAY_INDEX[b]
      const out = []
      // Walk Mon-first; wrap around when end < start so Sat..Tue
      // covers Sat Sun Mon Tue.
      let i = start
      while (true) {
        out.push(SHORT_DAY[(i + 1) % 7]) // SHORT_DAY is Sun-first, our index is Mon-first
        if (i === end) break
        i = (i + 1) % 7
        if (out.length > 7) break
      }
      return out
    }
  }
  // Single token: full ("monday") or 3-letter ("mon").
  const key = raw.slice(0, 3)
  if (key in WEEKDAY_INDEX) return [SHORT_DAY[(WEEKDAY_INDEX[key] + 1) % 7]]
  return []
}

function ruleMatches(rule, date) {
  if (rule.rule_type === 'weekly') {
    const expanded = rule.args.flatMap(expandWeeklyArg)
    const today = SHORT_DAY[date.getUTCDay()]
    return expanded.includes(today)
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
