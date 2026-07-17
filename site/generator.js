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
const WASM_VERSION = '2026-07-15f'

import init, * as wasm from './wasm/croniq_config_wasm.js?v=2026-07-15f'

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
  key: 'reports:daily',
  mode: 'interval',
  interval: { count: 5, unit: 'minutes' },
  daily: { hour: 9, minute: 0 },
  weekdays: { days: ['monday', 'tuesday', 'wednesday', 'thursday', 'friday'], hour: 9, minute: 0 },
  monthly: { ordinals: ['1st'], hour: 3, minute: 0 },
  once: { at: '2026-12-31T23:00:00Z' },
  // Optional job-level options (Phase 1). Empty/unset fields are omitted.
  opts: {
    description: '',
    timeout: '',
    retry: { enabled: false, strategy: 'exponential', max: 3, base: '5s', cap: '2m', jitter: 0.3, delay: '10s' },
    runnerRequire: '',
    runnerPrefer: '',
    tags: '',
    concurrency: 'default', // 'default' | 'singleton' | 'max_concurrent'
    maxConcurrent: 3,
    // Phase 2 — schedule constraints (recurring modes only) + execution.
    schedCalendar: '',
    schedTimezone: '',
    windowFrom: '',
    windowTo: '',
    notBefore: '',
    notAfter: '',
    executionMode: 'queued', // 'queued' | 'ephemeral'
    catchUp: 'default',      // 'default' | 'all' | 'latest' | 'none'
    queueTtl: '',
    maxQueueDepth: '',
    // Phase 3c — runner execution payload.
    runnerExec: { mode: 'none', command: '', args: '', workdir: '', user: '', env: '' },
  },
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
  syncRecurringVisibility()
  refreshSchedule()
})

// Per-mode field bindings — tiny wrappers that mutate schState then
// re-render. Each input's `change` event triggers a refresh.
function bindNumber(id, getter, setter) {
  const el = document.getElementById(id)
  el.value = getter()
  const min = el.min !== '' ? parseInt(el.min, 10) : 1
  el.addEventListener('input', () => {
    const raw = parseInt(el.value, 10)
    // Ignore empty/garbage input instead of coercing to 0 — that used to
    // silently emit `every 0 minutes`. Clamp valid input to the field min.
    if (Number.isNaN(raw)) return
    setter(Math.max(min, raw))
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

bindText('sch-key', () => schState.key, (v) => { schState.key = v })
bindNumber('sch-int-count', () => schState.interval.count, (v) => { schState.interval.count = v })
bindSelect('sch-int-unit', () => schState.interval.unit, (v) => { schState.interval.unit = v })
bindTime('sch-daily-time', () => schState.daily, (h, m) => { schState.daily.hour = h; schState.daily.minute = m })
bindTime('sch-wd-time', () => schState.weekdays, (h, m) => { schState.weekdays.hour = h; schState.weekdays.minute = m })
bindTime('sch-mth-time', () => schState.monthly, (h, m) => { schState.monthly.hour = h; schState.monthly.minute = m })
bindText('sch-once-at', () => schState.once.at, (v) => { schState.once.at = v })

// ── Job options (Phase 1) ───────────────────────────────────────────

const O = schState.opts
bindText('sch-opt-description', () => O.description, (v) => { O.description = v })
bindText('sch-opt-timeout', () => O.timeout, (v) => { O.timeout = v })
bindText('sch-opt-runner-require', () => O.runnerRequire, (v) => { O.runnerRequire = v })
bindText('sch-opt-runner-prefer', () => O.runnerPrefer, (v) => { O.runnerPrefer = v })
bindText('sch-opt-tags', () => O.tags, (v) => { O.tags = v })
bindText('sch-opt-retry-base', () => O.retry.base, (v) => { O.retry.base = v })
bindText('sch-opt-retry-cap', () => O.retry.cap, (v) => { O.retry.cap = v })
bindText('sch-opt-retry-delay', () => O.retry.delay, (v) => { O.retry.delay = v })
bindNumber('sch-opt-retry-max', () => O.retry.max, (v) => { O.retry.max = v })

// Jitter is a float in [0,1]; bindNumber is integer-only, so bind it raw.
const jitterEl = document.getElementById('sch-opt-retry-jitter')
jitterEl.value = O.retry.jitter
jitterEl.addEventListener('input', () => {
  const v = parseFloat(jitterEl.value)
  if (!Number.isNaN(v)) O.retry.jitter = v
  refreshSchedule()
})

// Retry enable toggle shows/hides the retry detail fields.
const retryEnabledEl = document.getElementById('sch-opt-retry-enabled')
const retryFieldsEl = document.getElementById('sch-opt-retry-fields')
retryEnabledEl.checked = O.retry.enabled
retryFieldsEl.hidden = !O.retry.enabled
retryEnabledEl.addEventListener('change', () => {
  O.retry.enabled = retryEnabledEl.checked
  retryFieldsEl.hidden = !O.retry.enabled
  refreshSchedule()
})

// Strategy select swaps exponential (base/cap/jitter) vs fixed (delay).
const retryStrategyEl = document.getElementById('sch-opt-retry-strategy')
const retryExpEl = document.getElementById('sch-opt-retry-exp')
const retryFixedEl = document.getElementById('sch-opt-retry-fixed')
function syncRetryStrategy() {
  const fixed = O.retry.strategy === 'fixed'
  retryExpEl.hidden = fixed
  retryFixedEl.hidden = !fixed
}
retryStrategyEl.value = O.retry.strategy
retryStrategyEl.addEventListener('change', () => {
  O.retry.strategy = retryStrategyEl.value
  syncRetryStrategy()
  refreshSchedule()
})
syncRetryStrategy()

// Concurrency select reveals the max-concurrent number field.
const concurrencyEl = document.getElementById('sch-opt-concurrency')
const maxcFieldEl = document.getElementById('sch-opt-maxc-field')
function syncConcurrency() {
  maxcFieldEl.hidden = O.concurrency !== 'max_concurrent'
}
concurrencyEl.value = O.concurrency
concurrencyEl.addEventListener('change', () => {
  O.concurrency = concurrencyEl.value
  syncConcurrency()
  refreshSchedule()
})
syncConcurrency()
bindNumber('sch-opt-maxc', () => O.maxConcurrent, (v) => { O.maxConcurrent = v })

// Phase 2 — schedule constraints + execution directives.
bindText('sch-opt-calendar', () => O.schedCalendar, (v) => { O.schedCalendar = v })
bindText('sch-opt-timezone', () => O.schedTimezone, (v) => { O.schedTimezone = v })
bindText('sch-opt-window-from', () => O.windowFrom, (v) => { O.windowFrom = v })
bindText('sch-opt-window-to', () => O.windowTo, (v) => { O.windowTo = v })
bindText('sch-opt-not-before', () => O.notBefore, (v) => { O.notBefore = v })
bindText('sch-opt-not-after', () => O.notAfter, (v) => { O.notAfter = v })
bindText('sch-opt-queue-ttl', () => O.queueTtl, (v) => { O.queueTtl = v })
bindText('sch-opt-max-queue', () => O.maxQueueDepth, (v) => { O.maxQueueDepth = v })
bindSelect('sch-opt-exec-mode', () => O.executionMode, (v) => { O.executionMode = v })
bindSelect('sch-opt-catch-up', () => O.catchUp, (v) => { O.catchUp = v })

// Runner command (Phase 3c).
const RE = O.runnerExec
bindText('sch-opt-re-command', () => RE.command, (v) => { RE.command = v })
bindText('sch-opt-re-args', () => RE.args, (v) => { RE.args = v })
bindText('sch-opt-re-workdir', () => RE.workdir, (v) => { RE.workdir = v })
bindText('sch-opt-re-user', () => RE.user, (v) => { RE.user = v })
bindText('sch-opt-re-env', () => RE.env, (v) => { RE.env = v })
const reModeEl = document.getElementById('sch-opt-re-mode')
const reFieldsEl = document.getElementById('sch-opt-re-fields')
const reCommandFieldEl = document.getElementById('sch-opt-re-command-field')
const reArgsFieldEl = document.getElementById('sch-opt-re-args-field')
function syncRunnerExec() {
  reFieldsEl.hidden = RE.mode === 'none'
  reCommandFieldEl.hidden = RE.mode !== 'shell'
  reArgsFieldEl.hidden = RE.mode !== 'exec'
}
reModeEl.value = RE.mode
reModeEl.addEventListener('change', () => { RE.mode = reModeEl.value; syncRunnerExec(); refreshSchedule() })
syncRunnerExec()

// The recurring-only constraints section is meaningless for once/disabled.
const recurringOptsEl = document.getElementById('sch-opt-recurring')
function syncRecurringVisibility() {
  recurringOptsEl.hidden = schState.mode === 'once' || schState.mode === 'disabled'
}
syncRecurringVisibility()

// Assemble the wasm `JobOptions` shape from the form state. Omits empty
// fields so the emitted block only carries what the user actually set.
function buildJobOptions() {
  const opts = {}
  if (O.description.trim()) opts.description = O.description.trim()
  if (O.timeout.trim()) opts.timeout = O.timeout.trim()
  if (O.retry.enabled) {
    const r = { strategy: O.retry.strategy }
    if (O.retry.max) r.max_attempts = O.retry.max
    if (O.retry.strategy === 'fixed') {
      if (O.retry.delay.trim()) r.delay = O.retry.delay.trim()
    } else {
      if (O.retry.base.trim()) r.base = O.retry.base.trim()
      if (O.retry.cap.trim()) r.cap = O.retry.cap.trim()
      if (typeof O.retry.jitter === 'number') r.jitter = O.retry.jitter
    }
    opts.retry = r
  }
  const req = O.runnerRequire.split(/[\s,]+/).filter(Boolean)
  const pref = O.runnerPrefer.split(/[\s,]+/).filter(Boolean)
  if (req.length) opts.runner_require = req
  if (pref.length) opts.runner_prefer = pref
  const tags = O.tags.split(/[\s,]+/).filter(Boolean)
  if (tags.length) opts.tags = tags
  if (O.concurrency === 'singleton') opts.concurrency = 'singleton'
  else if (O.concurrency === 'max_concurrent') opts.concurrency = String(O.maxConcurrent)

  // Recurring-only scheduling constraints — the schedule-options block is
  // invalid on once/disabled, so don't emit them there (the wasm bridge
  // drops them defensively too).
  const recurring = schState.mode !== 'once' && schState.mode !== 'disabled'
  if (recurring) {
    if (O.schedCalendar.trim()) opts.schedule_calendar = O.schedCalendar.trim()
    if (O.schedTimezone.trim()) opts.schedule_timezone = O.schedTimezone.trim()
    if (O.windowFrom && O.windowTo) opts.window = `${O.windowFrom}..${O.windowTo}`
    if (O.notBefore.trim()) opts.not_before = O.notBefore.trim()
    if (O.notAfter.trim()) opts.not_after = O.notAfter.trim()
  }
  // Execution directives apply in every mode. `queued` is the implicit
  // default, so only emit an explicit `ephemeral`.
  if (O.executionMode === 'ephemeral') opts.execution_mode = 'ephemeral'
  if (O.catchUp !== 'default') opts.catch_up = O.catchUp
  if (O.queueTtl.trim()) opts.queue_ttl = O.queueTtl.trim()
  if (O.maxQueueDepth) opts.max_queue_depth = parseInt(O.maxQueueDepth, 10)

  // Runner command payload. The wasm side omits it when there's no
  // command/args, so an incomplete draft simply produces no runner block.
  if (RE.mode !== 'none') {
    const re = { mode: RE.mode }
    if (RE.mode === 'shell') {
      if (RE.command.trim()) re.command = RE.command.trim()
    } else {
      const args = RE.args.split(/\s+/).filter(Boolean)
      if (args.length) re.args = args
    }
    if (RE.workdir.trim()) re.workdir = RE.workdir.trim()
    if (RE.user.trim()) re.user = RE.user.trim()
    const env = RE.env
      .split('\n')
      .map((l) => l.trim())
      .filter(Boolean)
      .map((l) => {
        const i = l.search(/\s/)
        return i === -1 ? { key: l, value: '' } : { key: l.slice(0, i), value: l.slice(i + 1).trim() }
      })
    if (env.length) re.env = env
    opts.runner_exec = re
  }
  return opts
}

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
  const payload = buildSchedulePayload()

  // Bare schedule line — drives the next-fires preview (and never throws).
  let line = ''
  try { line = wasm.formatSchedule(payload) } catch { line = '' }

  // Full, paste-ready `job <key> { … }` block (schedule + options) for
  // the output box + Copy. Throws on invalid input (bad key/duration) —
  // surface that as the validation error.
  let block = ''
  try {
    block = wasm.formatJobBlock(payload, schState.key, buildJobOptions())
  } catch (e) {
    schDslEl.textContent = ''
    schErrEl.hidden = false
    schErrEl.textContent = String(e)
    schFiresEl.textContent = ''
    return
  }
  schDslEl.textContent = block

  // Live next-fires preview, current UTC instant. The wasm crate's
  // next-fire path is UTC-only by design (see PR #55) — for the
  // preview that's exactly what we want.
  const now = new Date().toISOString().replace(/\.\d{3}Z$/, 'Z')
  let result
  try {
    result = wasm.nextFires(line, now, 5)
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
  name: 'business-days',
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
const calNameEl = document.getElementById('cal-name')

calNameEl.value = calState.name
calNameEl.addEventListener('input', () => { calState.name = calNameEl.value; refreshCalendar() })

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
    if (rule.rule_type === 'timezone') {
      // `timezone` is a bare directive — include/exclude is meaningless
      // (and prefixing it produced the invalid `include timezone …`).
      // Hide but keep the grid cell so the row layout stays aligned.
      action.disabled = true
      action.style.visibility = 'hidden'
      action.setAttribute('aria-hidden', 'true')
    }

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

  // Full, paste-ready `calendar <name> { … }` block for the output box +
  // Copy. `formatCalendarBlock` parses internally, so a throw here is
  // also our validation signal — skip the grid and show the diagnostic,
  // leaving the old grid visible until the input is fixed.
  let block = ''
  try {
    block = wasm.formatCalendarBlock(calState.rules, calState.name)
  } catch (e) {
    calDslEl.textContent = ''
    calErrEl.hidden = false
    calErrEl.textContent = String(e)
    return
  }
  calDslEl.textContent = block

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

// ── Config panel (top-level blocks) ─────────────────────────────────

// Declarative form schema per block. `type` defaults to a text input;
// `select` renders a dropdown (first option "" = unset); `multi` splits
// the value on whitespace into multiple directive args. `vars` is a
// freeform `key value` list (one per line) rather than fixed fields.
const CONFIG_SCHEMA = {
  server: [
    { key: 'listen', label: 'Listen address', placeholder: ':4000' },
    { key: 'data_dir', label: 'Data directory', placeholder: '/var/lib/croniq' },
    { key: 'db', label: 'Database', placeholder: 'sqlite' },
    { key: 'app_url', label: 'App URL', placeholder: 'https://cron.example.com' },
  ],
  smtp: [
    { key: 'host', label: 'Host', placeholder: 'smtp.example.com' },
    { key: 'port', label: 'Port', type: 'number', placeholder: '587' },
    { key: 'security', label: 'Security', type: 'select', options: ['', 'starttls', 'tls', 'none'] },
    { key: 'from', label: 'From', placeholder: 'Croniq <noreply@example.com>' },
  ],
  pull_api: [
    { key: 'listen', label: 'Listen address', placeholder: ':4000' },
    { key: 'lease_ttl', label: 'Lease TTL', placeholder: '60s' },
    { key: 'trigger_dedup_window', label: 'Trigger dedup window', placeholder: '10m' },
  ],
  mcp: [
    { key: 'enabled', label: 'Enabled', type: 'select', options: ['', 'true', 'false'] },
    { key: 'allowed_hosts', label: 'Allowed hosts', multi: true, placeholder: 'space-separated hosts' },
  ],
  policy: [
    { key: 'dsl_adopt_on_mutate', label: 'Adopt DSL on mutate', type: 'select', options: ['', 'true', 'false'] },
  ],
  oidc: [
    { key: 'issuer', label: 'Issuer', placeholder: 'https://id.example.com' },
    { key: 'client_id', label: 'Client ID', placeholder: 'croniq' },
    { key: 'redirect_url', label: 'Redirect URL', placeholder: 'https://cron.example.com/oidc/callback' },
    { key: 'default_role', label: 'Default role', placeholder: 'viewer' },
    { key: 'provider_name', label: 'Provider name' },
    { key: 'post_login_redirect', label: 'Post-login redirect' },
  ],
  observability: [
    { sub: 'log', label: 'Logging', fields: [
      { key: 'level', label: 'Level', type: 'select', options: ['', 'trace', 'debug', 'info', 'warn', 'error'] },
      { key: 'format', label: 'Format', type: 'select', options: ['', 'json', 'text'] },
      { key: 'output', label: 'Output', placeholder: 'stderr' },
    ] },
    { sub: 'metrics', label: 'Metrics', fields: [
      { key: 'listen', label: 'Listen', placeholder: ':9900' },
      { key: 'path', label: 'Path', placeholder: '/metrics' },
    ] },
  ],
  defaults: [
    { key: 'timezone', label: 'Timezone', placeholder: 'Europe/Vienna' },
    { key: 'timeout', label: 'Timeout', placeholder: '5m' },
    { key: 'execution_mode', label: 'Execution mode', type: 'select', options: ['', 'queued', 'ephemeral'] },
    { key: 'catch_up', label: 'Catch-up', type: 'select', options: ['', 'all', 'latest', 'none'] },
    { sub: 'retry', label: 'Retry', qualifier: { options: ['exponential', 'fixed'] }, fields: [
      { key: 'max_attempts', label: 'Max attempts', type: 'number' },
      { key: 'base', label: 'Base', placeholder: '2s' },
      { key: 'cap', label: 'Cap', placeholder: '30s' },
      { key: 'jitter', label: 'Jitter', type: 'number' },
      { key: 'delay', label: 'Delay', placeholder: '10s' },
    ] },
    { sub: 'dead_letter', label: 'Dead letter', fields: [
      { key: 'retention', label: 'Retention', placeholder: '30d' },
      { key: 'replay_max_age', label: 'Replay max age', placeholder: '7d' },
      { key: 'operator_hint', label: 'Operator hint' },
    ] },
  ],
  alerts: 'alerts',
  vars: 'freeform',
}

const cfgState = {
  block: 'server',
  values: {},
  varsText: 'default_tz Europe/Vienna',
  alerts: {
    channels: [{ name: 'oncall', kind: 'shell', shell: '/usr/bin/page-oncall.sh', webhook: '', timeout: '', email: '' }],
    rules: [{ name: 'prod-failures', when: 'job_failed', jobKey: 'billing:*', channels: 'oncall' }],
  },
}

const cfgBlockEl = document.getElementById('cfg-block')
const cfgFieldsEl = document.getElementById('cfg-fields')
const cfgDslEl = document.getElementById('cfg-dsl')
const cfgErrEl = document.getElementById('cfg-error')

function cfgVals() {
  if (!cfgState.values[cfgState.block]) cfgState.values[cfgState.block] = {}
  return cfgState.values[cfgState.block]
}

function renderConfigFields() {
  cfgFieldsEl.innerHTML = ''
  const schema = CONFIG_SCHEMA[cfgState.block]
  if (schema === 'alerts') {
    renderAlertsEditor()
    return
  }
  if (schema === 'freeform') {
    const field = document.createElement('div')
    field.className = 'field'
    const label = document.createElement('label')
    label.setAttribute('for', 'cfg-vars')
    label.textContent = 'Variables (one per line: name value)'
    const ta = document.createElement('textarea')
    ta.id = 'cfg-vars'
    ta.rows = 5
    ta.spellcheck = false
    ta.value = cfgState.varsText
    ta.addEventListener('input', () => { cfgState.varsText = ta.value; refreshConfig() })
    field.appendChild(label)
    field.appendChild(ta)
    cfgFieldsEl.appendChild(field)
    return
  }
  const vals = cfgVals()
  schema.forEach((entry) => {
    if (entry.sub) {
      // Sub-block: a small heading, an optional qualifier select, then
      // its leaf fields keyed as `<sub>.<field>`.
      const head = document.createElement('div')
      head.className = 'cfg-subhead'
      head.textContent = entry.label
      cfgFieldsEl.appendChild(head)
      if (entry.qualifier) {
        const qKey = `${entry.sub}.__q`
        if (vals[qKey] === undefined) vals[qKey] = entry.qualifier.options[0]
        renderLeaf(
          { key: qKey, label: 'Strategy', type: 'select', options: entry.qualifier.options },
          vals,
        )
      }
      entry.fields.forEach((f) => renderLeaf({ ...f, key: `${entry.sub}.${f.key}` }, vals))
    } else {
      renderLeaf(entry, vals)
    }
  })
}

// Render one leaf input (text/number/select) bound to `vals[f.key]`.
function renderLeaf(f, vals) {
  const field = document.createElement('div')
  field.className = 'field'
  const label = document.createElement('label')
  const id = `cfg-${cfgState.block}-${f.key.replace(/\./g, '-')}`
  label.setAttribute('for', id)
  label.textContent = f.label
  field.appendChild(label)
  let input
  if (f.type === 'select') {
    input = document.createElement('select')
    f.options.forEach((o) => {
      const opt = document.createElement('option')
      opt.value = o
      opt.textContent = o === '' ? '(unset)' : o
      input.appendChild(opt)
    })
  } else {
    input = document.createElement('input')
    input.type = f.type === 'number' ? 'number' : 'text'
    if (f.placeholder) input.placeholder = f.placeholder
    input.spellcheck = false
  }
  input.id = id
  input.value = vals[f.key] ?? ''
  const evt = f.type === 'select' ? 'change' : 'input'
  input.addEventListener(evt, () => { vals[f.key] = input.value; refreshConfig() })
  field.appendChild(input)
  cfgFieldsEl.appendChild(field)
}

// Repeatable channel/rule editor for the alerts block.
function renderAlertsEditor() {
  const A = cfgState.alerts
  const head = (t) => { const h = document.createElement('div'); h.className = 'cfg-subhead'; h.textContent = t; return h }
  const field = (labelText, input) => {
    const f = document.createElement('div'); f.className = 'field'
    const l = document.createElement('label'); l.textContent = labelText
    f.appendChild(l); f.appendChild(input); return f
  }
  const textInput = (val, ph, on) => {
    const i = document.createElement('input'); i.type = 'text'; i.value = val || ''
    if (ph) i.placeholder = ph; i.spellcheck = false
    i.addEventListener('input', () => { on(i.value); refreshConfig() }); return i
  }
  const selectInput = (val, opts, on) => {
    const s = document.createElement('select')
    opts.forEach((o) => { const op = document.createElement('option'); op.value = o; op.textContent = o; s.appendChild(op) })
    s.value = val
    s.addEventListener('change', () => { on(s.value); refreshConfig() }); return s
  }
  const removeBtn = (on) => {
    const b = document.createElement('button'); b.type = 'button'; b.className = 'rule-remove'
    b.textContent = '×'; b.addEventListener('click', on); return b
  }
  const addBtn = (label, on) => {
    const b = document.createElement('button'); b.type = 'button'; b.className = 'rule-add'
    b.textContent = label; b.addEventListener('click', on); return b
  }

  cfgFieldsEl.appendChild(head('Channels'))
  A.channels.forEach((c, idx) => {
    const row = document.createElement('div'); row.className = 'rule-row'
    const h = document.createElement('div'); h.className = 'rule-row-head'
    h.appendChild(textInput(c.name, 'name', (v) => { c.name = v }))
    h.appendChild(selectInput(c.kind, ['shell', 'webhook', 'email'], (v) => { c.kind = v; renderConfigFields() }))
    h.appendChild(removeBtn(() => { A.channels.splice(idx, 1); renderConfigFields(); refreshConfig() }))
    row.appendChild(h)
    if (c.kind === 'shell') {
      row.appendChild(field('Command', textInput(c.shell, '/usr/bin/page-oncall.sh', (v) => { c.shell = v })))
    } else if (c.kind === 'webhook') {
      row.appendChild(field('Webhook URL', textInput(c.webhook, 'https://hooks.example.com/x', (v) => { c.webhook = v })))
      row.appendChild(field('Timeout (optional)', textInput(c.timeout, '10s', (v) => { c.timeout = v })))
    } else {
      row.appendChild(field('Recipients (space-separated)', textInput(c.email, 'a@example.com b@example.com', (v) => { c.email = v })))
    }
    cfgFieldsEl.appendChild(row)
  })
  cfgFieldsEl.appendChild(addBtn('+ Add channel', () => {
    A.channels.push({ name: '', kind: 'shell', shell: '', webhook: '', timeout: '', email: '' })
    renderConfigFields(); refreshConfig()
  }))

  cfgFieldsEl.appendChild(head('Rules'))
  A.rules.forEach((r, idx) => {
    const row = document.createElement('div'); row.className = 'rule-row'
    const h = document.createElement('div'); h.className = 'rule-row-head'
    h.appendChild(textInput(r.name, 'name', (v) => { r.name = v }))
    h.appendChild(selectInput(r.when, ['job_failed', 'job_sla_missed', 'job_missed_fire'], (v) => { r.when = v }))
    h.appendChild(removeBtn(() => { A.rules.splice(idx, 1); renderConfigFields(); refreshConfig() }))
    row.appendChild(h)
    row.appendChild(field('Job key glob', textInput(r.jobKey, 'billing:*', (v) => { r.jobKey = v })))
    row.appendChild(field('Channels (space-separated names)', textInput(r.channels, 'oncall', (v) => { r.channels = v })))
    cfgFieldsEl.appendChild(row)
  })
  cfgFieldsEl.appendChild(addBtn('+ Add rule', () => {
    A.rules.push({ name: '', when: 'job_failed', jobKey: '', channels: '' })
    renderConfigFields(); refreshConfig()
  }))
}

// Build the alerts directive tree (channel/rule sub-blocks with quoted names).
function buildAlertsDirectives() {
  const A = cfgState.alerts
  const dirs = []
  A.channels.forEach((c) => {
    const name = (c.name || '').trim()
    if (!name) return
    const children = []
    if (c.kind === 'shell') {
      const v = (c.shell || '').trim(); if (v) children.push({ key: 'shell', args: [v] })
    } else if (c.kind === 'webhook') {
      const v = (c.webhook || '').trim(); if (v) children.push({ key: 'webhook', args: [v] })
      const t = (c.timeout || '').trim(); if (t) children.push({ key: 'timeout', args: [t] })
    } else {
      const em = (c.email || '').split(/\s+/).filter(Boolean); if (em.length) children.push({ key: 'email', args: em })
    }
    if (children.length === 0) return
    dirs.push({ key: 'channel', qualifier: name, quote_qualifier: true, children })
  })
  A.rules.forEach((r) => {
    const name = (r.name || '').trim()
    if (!name) return
    const children = [{ key: 'when', args: [r.when] }]
    const jk = (r.jobKey || '').trim(); if (jk) children.push({ key: 'job_key', args: [jk] })
    const ch = (r.channels || '').split(/\s+/).filter(Boolean); if (ch.length) children.push({ key: 'channels', args: ch })
    dirs.push({ key: 'rule', qualifier: name, quote_qualifier: true, children })
  })
  return dirs
}

// Turn the current block's form state into the wasm directive tree.
function buildConfigDirectives() {
  if (cfgState.block === 'alerts') return buildAlertsDirectives()
  if (cfgState.block === 'vars') {
    return cfgState.varsText
      .split('\n')
      .map((l) => l.trim())
      .filter(Boolean)
      .map((l) => {
        const parts = l.split(/\s+/)
        return { key: parts[0], args: parts.slice(1) }
      })
      .filter((d) => d.key)
  }
  const vals = cfgVals()
  const schema = CONFIG_SCHEMA[cfgState.block]
  const dirs = []
  const leaf = (key, f) => {
    const v = (vals[key] ?? '').trim()
    if (!v) return null
    return { key: f.key, args: f.multi ? v.split(/\s+/) : [v] }
  }
  schema.forEach((entry) => {
    if (entry.sub) {
      const children = []
      entry.fields.forEach((f) => {
        const d = leaf(`${entry.sub}.${f.key}`, f)
        if (d) children.push(d)
      })
      if (children.length === 0) return
      const dir = { key: entry.sub, children }
      if (entry.qualifier) dir.qualifier = vals[`${entry.sub}.__q`] || entry.qualifier.options[0]
      dirs.push(dir)
    } else {
      const d = leaf(entry.key, entry)
      if (d) dirs.push(d)
    }
  })
  return dirs
}

async function refreshConfig() {
  await ensureWasm()
  cfgErrEl.hidden = true
  const dirs = buildConfigDirectives()
  if (dirs.length === 0) {
    cfgDslEl.textContent = ''
    cfgErrEl.hidden = false
    cfgErrEl.style.color = 'var(--fg-muted)'
    cfgErrEl.textContent = 'Fill at least one field to generate the block.'
    return
  }
  cfgErrEl.style.color = ''
  try {
    cfgDslEl.textContent = wasm.formatTopLevelBlock(cfgState.block, dirs)
  } catch (e) {
    cfgDslEl.textContent = ''
    cfgErrEl.hidden = false
    cfgErrEl.textContent = String(e)
  }
}

cfgBlockEl.addEventListener('change', () => {
  cfgState.block = cfgBlockEl.value
  renderConfigFields()
  refreshConfig()
})

document.getElementById('cfg-copy').addEventListener('click', async (e) => {
  if (!cfgDslEl.textContent) return
  await navigator.clipboard.writeText(cfgDslEl.textContent)
  const btn = e.currentTarget
  const orig = btn.textContent
  btn.textContent = 'Copied!'
  setTimeout(() => { btn.textContent = orig }, 1200)
})

// ── Bootstrap ──────────────────────────────────────────────────────

renderRuleEditor()
refreshSchedule()
refreshCalendar()
renderConfigFields()
refreshConfig()
