<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from 'vue'

/**
 * The demo console on the sign-in stage.
 *
 * Types a `croniq` command, streams what running it would print, holds, then
 * moves to the next. It is the one thing on this page that shows what the
 * product *is* rather than asserting it — and it was dropped from the first
 * Vue login on the grounds that it was decoration, which was not my call to
 * make.
 *
 * Every command below is a real subcommand of the CLI
 * (`crates/croniq-cli/src/main.rs`). The output lines are illustrative but
 * shaped after what that command is responsible for — nothing here implies a
 * feature the binary does not ship, which is the rule that keeps a demo from
 * becoming a lie.
 */
interface DemoLine {
  level: 'info' | 'ok'
  text: string
}
interface Demo {
  cmd: string
  arg?: string
  output: DemoLine[]
}

const DEMOS: Demo[] = [
  {
    cmd: 'croniq quickstart',
    output: [
      { level: 'info', text: 'wrote ./Croniqfile with a sample heartbeat job' },
      { level: 'info', text: 'created ./.data/croniq.db · admin user provisioned' },
      { level: 'ok', text: 'next: croniq-server --config Croniqfile --data-dir ./.data' },
    ],
  },
  {
    cmd: 'croniq validate',
    arg: 'Croniqfile',
    output: [
      { level: 'info', text: 'parsed Croniqfile · 3 jobs · 1 calendar · 1 runner pool' },
      { level: 'info', text: 'no scheduling collisions · all rules reachable' },
      { level: 'ok', text: 'Croniqfile is valid' },
    ],
  },
  {
    cmd: 'croniq trigger',
    arg: 'demo:heartbeat',
    output: [
      { level: 'info', text: 'POST /v1/trigger → execution ex_5a8c2244 queued' },
      { level: 'info', text: 'claimed by runner shell-runner-7b31d0ee' },
      { level: 'ok', text: 'completed in 1.4s · exit 0' },
    ],
  },
  {
    cmd: 'croniq convert',
    arg: "'0 9 * * 1-5'",
    output: [
      { level: 'info', text: 'parsed as 5-field standard cron' },
      { level: 'ok', text: 'DSL: every weekday at 09:00' },
    ],
  },
  {
    cmd: 'croniq status',
    output: [
      { level: 'info', text: 'http://localhost:4000 · 2 runners online · 0 stale' },
      { level: 'info', text: 'queue depth 0 · 12 jobs registered' },
      { level: 'ok', text: 'scheduler healthy' },
    ],
  },
]

const TYPE_CHAR_MS = 38
const PAUSE_AFTER_TYPING_MS = 380
const OUTPUT_LINE_MS = 320
const HOLD_MS = 5000
const CLEAR_MS = 220

type Phase = 'typing' | 'output' | 'hold' | 'clearing'

/**
 * Motion is the whole point of this component, so it is also the whole thing
 * to switch off. With `prefers-reduced-motion` it renders one completed demo
 * and never ticks — no timers at all, rather than fast animation.
 */
const reducedMotion =
  typeof window !== 'undefined' &&
  window.matchMedia('(prefers-reduced-motion: reduce)').matches

const index = ref(0)
const typed = ref('')
const shown = ref(0)
const phase = ref<Phase>('typing')
const paused = ref(false)

const demo = computed(() => DEMOS[index.value]!)
const fullCmd = computed(() => (demo.value.arg ? `${demo.value.cmd} ${demo.value.arg}` : demo.value.cmd))

if (reducedMotion) {
  typed.value = fullCmd.value
  shown.value = demo.value.output.length
  phase.value = 'hold'
}

let timer: ReturnType<typeof setTimeout> | undefined
/**
 * Time already spent holding, so hovering away mid-countdown resumes where it
 * stopped rather than restarting the full five seconds.
 */
let heldFor = 0

function stop() {
  clearTimeout(timer)
  timer = undefined
}

function tick() {
  if (reducedMotion) return
  stop()

  if (phase.value === 'typing') {
    if (typed.value.length < fullCmd.value.length) {
      timer = setTimeout(() => {
        typed.value = fullCmd.value.slice(0, typed.value.length + 1)
      }, TYPE_CHAR_MS)
      return
    }
    timer = setTimeout(() => (phase.value = 'output'), PAUSE_AFTER_TYPING_MS)
    return
  }

  if (phase.value === 'output') {
    if (shown.value < demo.value.output.length) {
      timer = setTimeout(() => (shown.value += 1), OUTPUT_LINE_MS)
      return
    }
    heldFor = 0
    phase.value = 'hold'
    return
  }

  if (phase.value === 'hold') {
    // The only phase that honours a pause. Typing and output ticks are too
    // short for a hover to be useful, and clearing is deliberately quick.
    if (paused.value) return
    const startedAt = Date.now()
    const remaining = Math.max(0, HOLD_MS - heldFor)
    timer = setTimeout(() => (phase.value = 'clearing'), remaining)
    // Recorded on the way out, in `stop`'s place — see the watcher below.
    heldStartedAt = startedAt
    return
  }

  timer = setTimeout(() => {
    typed.value = ''
    shown.value = 0
    index.value = (index.value + 1) % DEMOS.length
    phase.value = 'typing'
  }, CLEAR_MS)
}

let heldStartedAt = 0

watch(paused, (isPaused) => {
  if (phase.value !== 'hold') return
  if (isPaused) {
    heldFor = Math.min(HOLD_MS, heldFor + (Date.now() - heldStartedAt))
    stop()
  } else {
    tick()
  }
})

watch([phase, typed, shown, index], tick, { immediate: true })
onScopeDispose(stop)

const cursorVisible = computed(
  () => phase.value === 'typing' && typed.value.length < fullCmd.value.length,
)

const statusLabel = computed(() => {
  if (reducedMotion) return 'example'
  switch (phase.value) {
    case 'typing':
      return 'typing'
    case 'output':
      return 'running'
    case 'hold':
      return paused.value ? 'paused' : 'idle'
    default:
      return ''
  }
})
</script>

<template>
  <!--
    `aria-hidden`: this is an illustration that types at its own pace. A screen
    reader reading it would be read a stream of half-finished commands, and
    nothing here is information the reader needs to sign in.
  -->
  <div
    class="cq-console overflow-hidden rounded-xl border border-white/10 bg-[oklch(0.16_0.02_265)] shadow-lg"
    aria-hidden="true"
    @mouseenter="paused = true"
    @mouseleave="paused = false"
  >
    <div class="flex items-center gap-2 border-b border-white/10 px-3 py-2">
      <span class="size-2.5 rounded-full bg-[oklch(0.65_0.18_25)]" />
      <span class="size-2.5 rounded-full bg-[oklch(0.78_0.16_75)]" />
      <span class="size-2.5 rounded-full bg-[oklch(0.70_0.16_145)]" />
      <span class="ml-2 font-mono text-xs text-white/40">~ croniq · live demo</span>
      <span
        v-if="statusLabel"
        class="ml-auto flex items-center gap-1.5 font-mono text-xs text-white/40"
      >
        <span
          class="size-1.5 rounded-full"
          :class="phase === 'output' ? 'bg-[oklch(0.70_0.16_145)]' : 'bg-white/30'"
        />
        {{ statusLabel }}
      </span>
    </div>

    <div
      class="h-44 px-3 py-2.5 font-mono text-xs transition-opacity duration-200"
      :class="phase === 'clearing' && 'opacity-0'"
    >
      <p>
        <span class="text-[oklch(0.70_0.16_145)]">$</span>
        <span class="ml-2 text-white/90">{{ typed }}</span>
        <span
          v-if="cursorVisible"
          class="cq-caret ml-0.5 inline-block h-3.5 w-[0.5ch] translate-y-0.5 bg-white/70"
        />
      </p>
      <p
        v-for="line in demo.output.slice(0, shown)"
        :key="line.text"
        class="mt-1 flex gap-2"
      >
        <span
          class="w-8 shrink-0"
          :class="line.level === 'ok' ? 'text-[oklch(0.70_0.16_145)]' : 'text-[oklch(0.72_0.12_250)]'"
        >{{ line.level }}</span>
        <span class="min-w-0 text-white/70">{{ line.text }}</span>
      </p>
    </div>
  </div>
</template>

<style scoped>
@keyframes cq-caret-blink {
  0%,
  45% {
    opacity: 1;
  }
  55%,
  100% {
    opacity: 0;
  }
}
.cq-caret {
  animation: cq-caret-blink 1s steps(1, end) infinite;
}
@media (prefers-reduced-motion: reduce) {
  .cq-caret {
    animation: none;
  }
}
</style>
