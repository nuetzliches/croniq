<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { formatCalendarRules, parseCalendarRules, type CalendarRulePayload } from '~/lib/croniq-dsl'

/**
 * Calendar rules, built rather than typed.
 *
 * The rules are formatted and parsed by `croniq-config` compiled to wasm — the
 * same crate the server loads a Croniqfile with. That is the point of the
 * exercise: a builder backed by its own hand-written parser would agree with
 * the server right up until the day it didn't, and the disagreement would show
 * up as a job that quietly fired on a holiday.
 *
 * Every rule type produces the same `args: string[]`, so the wasm call is one
 * call whatever the reader picked.
 */
const props = defineProps<{
  /** Starting rules. Parsed out of stored DSL by the form above. */
  initial?: CalendarRulePayload[]
}>()

const emit = defineEmits<{
  /** The formatted DSL, on every valid change. This is what gets saved. */
  change: [dsl: string]
  /** Diagnostics from the parse pass, or null when the rules are clean. */
  error: [message: string | null]
}>()

const RULE_TYPES = ['weekly', 'window', 'monthly', 'annual', 'timezone'] as const
type RuleType = (typeof RULE_TYPES)[number]

const RULE_TYPE_LABELS: Record<RuleType, string> = {
  weekly: 'Weekdays',
  window: 'Time window',
  monthly: 'Days of month',
  annual: 'Specific date',
  timezone: 'Timezone',
}

/** Beside the controls rather than in a tooltip: it stays put while you work. */
const RULE_TYPE_HINTS: Record<RuleType, string> = {
  weekly: 'Days when this rule applies.',
  window: "Hour range, read on the calendar's own timezone — not the job's.",
  monthly: 'Days of the month. "Last" means the last day, whatever its number.',
  annual: 'One date, without a year — it recurs every year.',
  timezone: 'IANA name, e.g. Europe/Berlin.',
}

const WEEKDAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'] as const
const WEEKDAY_PRESETS = [
  { label: 'Weekdays', days: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri'] },
  { label: 'Weekend', days: ['Sat', 'Sun'] },
  { label: 'Every day', days: [...WEEKDAYS] },
]

const ORDINALS = [...Array.from({ length: 31 }, (_, i) => String(i + 1)), 'last']
const MONTHLY_PRESETS = [
  { label: '1st', days: ['1'] },
  { label: '15th', days: ['15'] },
  { label: '1st + 15th', days: ['1', '15'] },
  { label: 'Last day', days: ['last'] },
]
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

/**
 * A copy, field by field.
 *
 * Not `structuredClone`: the seed comes back from wasm through
 * `serde_wasm_bindgen`, and those objects are not structured-cloneable —
 * cloning them throws and takes the whole component render with it. Copying
 * the three fields this type actually has is both safe and honest about what
 * is being carried across.
 */
function copyRules(source: CalendarRulePayload[]): CalendarRulePayload[] {
  return source.map((rule) => ({
    action: rule.action,
    rule_type: rule.rule_type,
    args: [...rule.args],
  }))
}

const rules = ref<CalendarRulePayload[]>(
  props.initial?.length
    ? copyRules(props.initial)
    : [
        { action: 'include', rule_type: 'weekly', args: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri'] },
        { action: 'exclude', rule_type: 'annual', args: ['12-25'] },
      ],
)

const dsl = ref('')

/**
 * Format, then parse what was formatted.
 *
 * The format direction produces what gets saved. The parse pass back is what
 * catches a rule whose arguments are the wrong shape — an annual rule holding
 * words instead of MM-DD — before the reader presses save rather than after.
 */
let generation = 0
watch(
  rules,
  async (current) => {
    const mine = ++generation
    try {
      const formatted = await formatCalendarRules(current)
      if (mine !== generation) return
      dsl.value = formatted
      emit('change', formatted)
      const parsed = await parseCalendarRules(formatted)
      if (mine !== generation) return
      emit('error', parsed.ok ? null : parsed.diagnostics.join('\n'))
    } catch (caught) {
      if (mine !== generation) return
      emit('error', caught instanceof Error ? caught.message : String(caught))
    }
  },
  { deep: true, immediate: true },
)

function addRule() {
  rules.value.push({ action: 'include', rule_type: 'weekly', args: [] })
}

function removeRule(index: number) {
  rules.value.splice(index, 1)
}

function setType(index: number, type: string) {
  // Arguments are type-specific; carrying them across would render DSL that
  // looks plausible and means something else.
  rules.value[index] = { ...rules.value[index]!, rule_type: type, args: [] }
}

function toggleAction(index: number) {
  const rule = rules.value[index]!
  rule.action = rule.action === 'include' ? 'exclude' : 'include'
}

/* ─── weekly ─────────────────────────────────────────────────────────────── */

/** Stored days may be `Mon` or `monday`; one shape in, one shape out. */
function normaliseDay(value: string): string | null {
  const match = WEEKDAYS.find((day) => day.toLowerCase() === value.toLowerCase().slice(0, 3))
  return match ?? null
}

function activeDays(rule: CalendarRulePayload): Set<string> {
  return new Set(rule.args.map(normaliseDay).filter((day): day is string => day !== null))
}

function toggleDay(index: number, day: string) {
  const rule = rules.value[index]!
  const active = activeDays(rule)
  if (active.has(day)) active.delete(day)
  else active.add(day)
  rule.args = WEEKDAYS.filter((candidate) => active.has(candidate))
}

/* ─── window ─────────────────────────────────────────────────────────────── */

function setWindow(index: number, slot: 0 | 1, value: string) {
  const rule = rules.value[index]!
  const next: [string, string] = [rule.args[0] ?? '', rule.args[1] ?? '']
  next[slot] = value
  // Drop empties rather than emit a half-open window mid-edit.
  rule.args = next[0] || next[1] ? next.filter(Boolean) : []
}

/* ─── monthly ────────────────────────────────────────────────────────────── */

function activeOrdinals(rule: CalendarRulePayload): Set<string> {
  return new Set(rule.args.map((arg) => arg.replace(/^(\d+)(st|nd|rd|th)$/i, '$1').toLowerCase()))
}

function toggleOrdinal(index: number, ordinal: string) {
  const rule = rules.value[index]!
  const active = activeOrdinals(rule)
  const key = ordinal.toLowerCase()
  if (active.has(key)) active.delete(key)
  else active.add(key)
  // Stable order, `last` at the end — so the DSL does not churn between
  // renders and a diff of two saves means something.
  rule.args = [...active].sort((a, b) => {
    if (a === 'last') return 1
    if (b === 'last') return -1
    return Number.parseInt(a, 10) - Number.parseInt(b, 10)
  })
}

/* ─── annual ─────────────────────────────────────────────────────────────── */

function annualParts(rule: CalendarRulePayload): { month: number; day: number } {
  const match = /^(\d{1,2})-(\d{1,2})$/.exec(rule.args[0] ?? '')
  return match
    ? { month: Number.parseInt(match[1]!, 10), day: Number.parseInt(match[2]!, 10) }
    : { month: 0, day: 0 }
}

function setAnnual(index: number, month: number, day: number) {
  const rule = rules.value[index]!
  rule.args =
    month && day
      ? [`${String(month).padStart(2, '0')}-${String(day).padStart(2, '0')}`]
      : []
}

const copied = ref(false)
async function copyDsl() {
  try {
    await navigator.clipboard.writeText(dsl.value)
    copied.value = true
    setTimeout(() => (copied.value = false), 1500)
  } catch {
    // The text is on screen and selectable.
  }
}

const typeItems = computed(() =>
  RULE_TYPES.map((type) => ({ label: `${RULE_TYPE_LABELS[type]} (${type})`, value: type })),
)
</script>

<template>
  <div class="flex flex-col gap-3">
    <div
      v-for="(rule, index) in rules"
      :key="index"
      class="flex flex-col gap-2 rounded-lg border border-default p-3"
    >
      <div class="flex items-center gap-2">
        <!-- A toggle rather than a dropdown: include adds days, exclude
             removes them, and +/− is how the rule reads aloud. -->
        <UButton
          :icon="rule.action === 'include' ? 'i-lucide-plus' : 'i-lucide-minus'"
          :color="rule.action === 'include' ? 'success' : 'error'"
          variant="subtle"
          size="xs"
          :aria-label="`Rule ${index + 1} is ${rule.action} — switch it`"
          :title="`${rule.action} — click to switch`"
          @click="toggleAction(index)"
        />
        <USelectMenu
          :model-value="rule.rule_type"
          :items="typeItems"
          value-key="value"
          size="xs"
          class="flex-1"
          :aria-label="`Rule ${index + 1} type`"
          @update:model-value="(value: string) => setType(index, value)"
        />
        <UButton
          icon="i-lucide-trash-2"
          color="error"
          variant="ghost"
          size="xs"
          :aria-label="`Remove rule ${index + 1}`"
          @click="removeRule(index)"
        />
      </div>

      <p class="text-xs text-muted">
        {{ RULE_TYPE_HINTS[rule.rule_type as RuleType] }}
      </p>

      <!-- weekly -->
      <div
        v-if="rule.rule_type === 'weekly'"
        class="flex flex-col gap-2"
      >
        <div class="grid grid-cols-7 gap-1">
          <UButton
            v-for="day in WEEKDAYS"
            :key="day"
            :variant="activeDays(rule).has(day) ? 'solid' : 'outline'"
            :color="activeDays(rule).has(day) ? 'primary' : 'neutral'"
            size="xs"
            class="justify-center font-mono"
            :aria-pressed="activeDays(rule).has(day)"
            @click="toggleDay(index, day)"
          >
            {{ day }}
          </UButton>
        </div>
        <div class="flex flex-wrap gap-2">
          <UButton
            v-for="preset in WEEKDAY_PRESETS"
            :key="preset.label"
            variant="link"
            color="neutral"
            size="xs"
            class="p-0"
            @click="rule.args = [...preset.days]"
          >
            {{ preset.label }}
          </UButton>
        </div>
      </div>

      <!-- window -->
      <div
        v-else-if="rule.rule_type === 'window'"
        class="flex items-center gap-2"
      >
        <UInput
          type="time"
          :model-value="rule.args[0] ?? ''"
          size="xs"
          class="w-28"
          aria-label="Window start"
          @update:model-value="(value: string) => setWindow(index, 0, value)"
        />
        <span class="text-xs text-muted">to</span>
        <UInput
          type="time"
          :model-value="rule.args[1] ?? ''"
          size="xs"
          class="w-28"
          aria-label="Window end"
          @update:model-value="(value: string) => setWindow(index, 1, value)"
        />
      </div>

      <!-- monthly -->
      <div
        v-else-if="rule.rule_type === 'monthly'"
        class="flex flex-col gap-2"
      >
        <div class="grid grid-cols-8 gap-1">
          <UButton
            v-for="ordinal in ORDINALS"
            :key="ordinal"
            :variant="activeOrdinals(rule).has(ordinal.toLowerCase()) ? 'solid' : 'outline'"
            :color="activeOrdinals(rule).has(ordinal.toLowerCase()) ? 'primary' : 'neutral'"
            size="xs"
            class="justify-center px-0 font-mono"
            :aria-pressed="activeOrdinals(rule).has(ordinal.toLowerCase())"
            @click="toggleOrdinal(index, ordinal)"
          >
            {{ ordinal }}
          </UButton>
        </div>
        <div class="flex flex-wrap gap-2">
          <UButton
            v-for="preset in MONTHLY_PRESETS"
            :key="preset.label"
            variant="link"
            color="neutral"
            size="xs"
            class="p-0"
            @click="rule.args = [...preset.days]"
          >
            {{ preset.label }}
          </UButton>
        </div>
      </div>

      <!-- annual -->
      <div
        v-else-if="rule.rule_type === 'annual'"
        class="flex items-center gap-2"
      >
        <USelectMenu
          :model-value="annualParts(rule).month || undefined"
          :items="MONTHS.map((label, i) => ({ label, value: i + 1 }))"
          value-key="value"
          size="xs"
          class="w-32"
          placeholder="Month…"
          aria-label="Month"
          @update:model-value="(value: number) => setAnnual(index, value, annualParts(rule).day)"
        />
        <UInput
          type="number"
          :min="1"
          :max="31"
          :model-value="annualParts(rule).day || ''"
          size="xs"
          class="w-20"
          placeholder="Day"
          aria-label="Day of month"
          @update:model-value="
            (value: string) => setAnnual(index, annualParts(rule).month, Number(value) || 0)
          "
        />
      </div>

      <!-- timezone -->
      <UInput
        v-else-if="rule.rule_type === 'timezone'"
        :model-value="rule.args[0] ?? ''"
        size="xs"
        class="font-mono"
        placeholder="Europe/Berlin"
        aria-label="Timezone"
        @update:model-value="(value: string) => (rule.args = value.trim() ? [value.trim()] : [])"
      />
    </div>

    <UButton
      icon="i-lucide-plus"
      variant="outline"
      color="neutral"
      size="xs"
      block
      class="border-dashed"
      @click="addRule"
    >
      Add rule
    </UButton>

    <div>
      <div class="mb-1 flex items-center justify-between">
        <p class="cq-label">
          Resulting DSL
        </p>
        <UButton
          v-if="dsl"
          :icon="copied ? 'i-lucide-check' : 'i-lucide-copy'"
          color="neutral"
          variant="ghost"
          size="xs"
          aria-label="Copy the calendar DSL"
          @click="copyDsl"
        />
      </div>
      <!-- `output` with a live region: the DSL changes as a consequence of
           what was just clicked, which is exactly what aria-live is for. -->
      <output
        class="block rounded-md border border-default bg-elevated p-2 font-mono text-xs break-words whitespace-pre-wrap"
        aria-live="polite"
      >{{ dsl || '—' }}</output>
    </div>
  </div>
</template>
