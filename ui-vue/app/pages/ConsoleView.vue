<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { useConsoleStream, type LogEvent } from '~/composables/useConsoleStream'

/**
 * The live console: the server's tracing feed, tailed.
 *
 * It kept its own screen rather than merging with run logs
 * (docs/ui-screen-inventory.md). Server tracing and a job's output are both
 * called "logs" and are different things; joining them creates exactly the
 * confusion that hurts while debugging.
 *
 * What changed against the React version is honesty about what a tail loses.
 * That one buffered silently while paused and silently dropped the oldest
 * events past its cap, so a console you looked away from told you nothing
 * about the gap. Both are counted and shown here.
 */
const {
  events,
  connected,
  forbidden,
  unavailable,
  dropped,
  paused,
  pendingCount,
  togglePause,
  clear,
} = useConsoleStream()

const LEVELS = ['debug', 'info', 'warn', 'error'] as const
const activeLevels = ref(new Set<string>(['info', 'warn', 'error']))
const search = ref('')

const filtered = computed(() => {
  const needle = search.value.trim().toLowerCase()
  return events.value.filter((event) => {
    if (!activeLevels.value.has(event.level)) return false
    if (!needle) return true
    return (
      event.message.toLowerCase().includes(needle) ||
      event.target.toLowerCase().includes(needle)
    )
  })
})

function toggleLevel(level: string) {
  const next = new Set(activeLevels.value)
  if (next.has(level)) next.delete(level)
  else next.add(level)
  activeLevels.value = next
}

/** Colour per level, in one place — the row uses it twice. */
function levelClass(level: string): string {
  switch (level) {
    case 'error':
      return 'text-error'
    case 'warn':
      return 'text-warning'
    case 'info':
      return 'text-info'
    case 'debug':
      return 'text-dimmed'
    default:
      return 'text-muted'
  }
}

/* ─── following the tail ─────────────────────────────────────────────────── */

const list = ref<HTMLElement | null>(null)
/**
 * Whether new events scroll into view.
 *
 * It follows until you scroll up, and then stops — reading something while the
 * view yanks itself to the bottom every few hundred milliseconds is the single
 * most irritating thing a live tail can do.
 */
const following = ref(true)

function onScroll() {
  const element = list.value
  if (!element) return
  const distance = element.scrollHeight - element.scrollTop - element.clientHeight
  following.value = distance < 40
}

watch(filtered, async () => {
  if (!following.value) return
  await nextTick()
  const element = list.value
  if (element) element.scrollTop = element.scrollHeight
})

function jumpToEnd() {
  following.value = true
  const element = list.value
  if (element) element.scrollTop = element.scrollHeight
}

/* ─── taking it away ─────────────────────────────────────────────────────── */

const copied = ref(false)

function asText(event: LogEvent): string {
  const fields = Object.entries(event.fields ?? {})
    .map(([key, value]) => `${key}=${typeof value === 'string' ? value : JSON.stringify(value)}`)
    .join(' ')
  return `${event.ts} ${event.level.toUpperCase().padEnd(5)} ${event.target} ${event.message}${
    fields ? ` ${fields}` : ''
  }`
}

async function copyAll() {
  try {
    await navigator.clipboard.writeText(filtered.value.map(asText).join('\n'))
    copied.value = true
    setTimeout(() => (copied.value = false), 1500)
  } catch {
    // Clipboard access can be refused; the export is the other way out.
  }
}

function downloadNdjson() {
  // NDJSON rather than the rendered text: whatever reads this next is a
  // program, and the fields survive as fields.
  const ndjson = filtered.value.map((event) => JSON.stringify(event)).join('\n')
  const blob = new Blob([ndjson], { type: 'application/x-ndjson' })
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = `croniq-console-${new Date().toISOString().replace(/[:.]/g, '-')}.ndjson`
  anchor.click()
  URL.revokeObjectURL(url)
}

const fieldText = (event: LogEvent): string =>
  Object.entries(event.fields ?? {})
    .map(([key, value]) => `${key}=${typeof value === 'string' ? value : JSON.stringify(value)}`)
    .join(' ')
</script>

<template>
  <div class="flex h-full min-h-0 flex-col gap-3">
    <div class="flex flex-wrap items-center gap-2">
      <div class="flex items-center gap-1">
        <UButton
          v-for="level in LEVELS"
          :key="level"
          :variant="activeLevels.has(level) ? 'subtle' : 'ghost'"
          color="neutral"
          size="xs"
          class="font-mono"
          :aria-pressed="activeLevels.has(level)"
          @click="toggleLevel(level)"
        >
          <span
            class="size-1.5 rounded-full"
            :class="activeLevels.has(level) ? levelClass(level).replace('text-', 'bg-') : 'bg-dimmed'"
            aria-hidden="true"
          />
          {{ level }}
        </UButton>
      </div>

      <UInput
        v-model="search"
        placeholder="Filter messages and targets…"
        icon="i-lucide-search"
        aria-label="Filter console events"
        size="sm"
        class="w-72"
      />

      <div class="ml-auto flex items-center gap-2">
        <span
          class="flex items-center gap-1.5 text-xs"
          :class="connected ? 'text-success' : 'text-muted'"
          role="status"
          aria-live="polite"
        >
          <UIcon
            :name="connected ? 'i-lucide-wifi' : 'i-lucide-wifi-off'"
            class="size-3.5"
          />
          {{ connected ? 'live' : forbidden || unavailable ? 'stopped' : 'reconnecting' }}
        </span>
        <span class="cq-num text-xs text-muted">
          {{ filtered.length }} / {{ events.length }}
        </span>
        <UButton
          :icon="paused ? 'i-lucide-play' : 'i-lucide-pause'"
          color="neutral"
          :variant="paused ? 'subtle' : 'ghost'"
          size="xs"
          :aria-label="paused ? 'Resume the console' : 'Pause the console'"
          @click="togglePause"
        >
          <!-- What is waiting is the thing a paused tail must not hide. -->
          {{ paused ? `Resume${pendingCount ? ` (${pendingCount})` : ''}` : 'Pause' }}
        </UButton>
        <UButton
          :icon="copied ? 'i-lucide-check' : 'i-lucide-copy'"
          color="neutral"
          variant="ghost"
          size="xs"
          aria-label="Copy the visible events"
          title="Copy what is on screen"
          @click="copyAll"
        />
        <UButton
          icon="i-lucide-download"
          color="neutral"
          variant="ghost"
          size="xs"
          aria-label="Download the visible events as NDJSON"
          title="Download as NDJSON"
          @click="downloadNdjson"
        />
        <UButton
          icon="i-lucide-trash-2"
          color="neutral"
          variant="ghost"
          size="xs"
          aria-label="Clear the console"
          title="Clear"
          @click="clear"
        />
      </div>
    </div>

    <UAlert
      v-if="forbidden"
      color="warning"
      variant="subtle"
      icon="i-lucide-lock"
      title="Administrator access required"
      description="The console tails the server's whole tracing feed, so it is admin-only. This is a settled answer rather than a connection problem — nothing is retrying in the background."
    />
    <UAlert
      v-else-if="unavailable"
      color="neutral"
      variant="subtle"
      icon="i-lucide-plug-zap"
      title="No console on this server"
      description="The server answered 503: this build has no live-console hub. Nothing is retrying."
    />

    <!-- A tail that has forgotten something says so. The React console
         dropped its oldest events silently, which is the one thing a log
         must not do quietly. -->
    <p
      v-if="dropped"
      class="cq-num text-xs text-warning"
      role="status"
    >
      {{ dropped }} older event{{ dropped === 1 ? '' : 's' }} dropped — the buffer holds the
      most recent 2000.
    </p>

    <div class="relative min-h-0 flex-1">
      <div
        ref="list"
        class="h-full overflow-auto rounded-lg border border-default bg-default font-mono text-xs"
        tabindex="0"
        role="log"
        aria-label="Server tracing events"
        @scroll="onScroll"
      >
        <AppEmpty
          v-if="filtered.length === 0"
          size="tight"
          icon="i-lucide-terminal"
          :title="
            forbidden
              ? 'Nothing to show'
              : events.length === 0
                ? 'Waiting for events'
                : 'Nothing matches'
          "
          :description="
            forbidden
              ? 'The stream did not open for this session.'
              : events.length === 0
                ? 'The stream is open and the server has not said anything yet. It will appear here as it happens.'
                : 'Every event so far is filtered out by the level buttons or the search.'
          "
        />

        <div
          v-for="(event, index) in filtered"
          v-else
          :key="index"
          class="flex items-start gap-3 border-l-2 px-3 py-1 hover:bg-elevated"
          :class="
            event.level === 'error'
              ? 'border-error'
              : event.level === 'warn'
                ? 'border-warning'
                : 'border-transparent'
          "
        >
          <!-- A coloured gutter as well as a level column: an error is found
               by scanning the left edge, not by reading every row. -->
          <span
            class="cq-num shrink-0 text-dimmed"
            :title="event.ts"
          >{{ event.ts.slice(11, 23) }}</span>
          <span
            class="w-11 shrink-0 font-medium uppercase"
            :class="levelClass(event.level)"
          >{{ event.level }}</span>
          <span class="w-52 shrink-0 truncate text-muted">{{ event.target }}</span>
          <span class="min-w-0 flex-1 break-words">
            {{ event.message }}
            <span
              v-if="fieldText(event)"
              class="ml-2 text-muted"
            >{{ fieldText(event) }}</span>
          </span>
        </div>
      </div>

      <!-- Only while detached: it says both that you are not following and
           how to get back, in one control. -->
      <UButton
        v-if="!following && filtered.length > 0"
        class="absolute right-4 bottom-4 shadow-lg"
        icon="i-lucide-arrow-down"
        size="xs"
        @click="jumpToEnd"
      >
        Follow
      </UButton>
    </div>
  </div>
</template>
