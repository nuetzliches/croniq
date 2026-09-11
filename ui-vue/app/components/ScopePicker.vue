<script setup lang="ts">
import { computed } from 'vue'

/**
 * Scopes, with presets.
 *
 * Mirrors `croniq_auth::Scope` (crates/croniq-auth/src/context.rs); keep the
 * grouping aligned with the README's *Scopes* table.
 *
 * The React form was twenty-odd checkboxes and nothing else, which is where
 * credentials go wrong: nobody reasons about twenty booleans, so people either
 * tick `admin` or tick roughly the right ones and find out later. The presets
 * name the three things anybody is actually trying to express, and the full
 * list stays underneath for the case that is genuinely bespoke.
 */
const selected = defineModel<string[]>({ required: true })

interface ScopeDef {
  value: string
  hint?: string
}

const SCOPE_GROUPS: { label: string; scopes: ScopeDef[] }[] = [
  { label: 'Admin', scopes: [{ value: 'admin', hint: 'Grants every scope below' }] },
  {
    label: 'Jobs',
    scopes: [
      { value: 'jobs:read' },
      { value: 'jobs:write' },
      { value: 'jobs:register', hint: 'POST /v1/jobs/register (runner SDK)' },
      { value: 'jobs:trigger', hint: 'POST /v1/trigger (manual fire)' },
    ],
  },
  { label: 'Schedules', scopes: [{ value: 'schedules:read' }, { value: 'schedules:write' }] },
  { label: 'Calendars', scopes: [{ value: 'calendars:read' }, { value: 'calendars:write' }] },
  {
    label: 'Executions',
    scopes: [{ value: 'executions:read', hint: 'Includes /executions/{id}/logs' }],
  },
  {
    label: 'Dead letters',
    scopes: [
      { value: 'dead-letters:read' },
      { value: 'dead-letters:write', hint: 'Replay and delete' },
    ],
  },
  {
    label: 'Runners',
    scopes: [
      { value: 'runners:read', hint: 'Includes /runners/stream (SSE)' },
      { value: 'runners:write' },
      { value: 'runners:heartbeat' },
    ],
  },
  {
    label: 'Runner pull-protocol',
    scopes: [
      { value: 'work:poll' },
      { value: 'work:ack' },
      { value: 'work:renew' },
      { value: 'work:events' },
    ],
  },
  {
    label: 'Identity / auth management',
    scopes: [{ value: 'api-clients:admin' }, { value: 'api-keys:admin' }],
  },
]

/**
 * The three shapes credentials actually take here.
 *
 * `Runner` is the one worth having: a runner needs the pull-protocol scopes
 * plus registration and heartbeat, and getting that set wrong by hand produces
 * a runner that connects and then silently fails to claim work.
 */
const PRESETS: { label: string; hint: string; scopes: string[] }[] = [
  {
    label: 'Read-only',
    hint: 'Everything the dashboard reads, nothing it writes.',
    scopes: [
      'jobs:read',
      'schedules:read',
      'calendars:read',
      'executions:read',
      'dead-letters:read',
      'runners:read',
    ],
  },
  {
    label: 'Runner',
    hint: 'What a worker needs to register, claim work and report back.',
    scopes: [
      'jobs:register',
      'runners:heartbeat',
      'work:poll',
      'work:ack',
      'work:renew',
      'work:events',
    ],
  },
  {
    label: 'Admin',
    hint: 'Every scope. Prefer a narrower set where one fits.',
    scopes: ['admin'],
  },
]

const chosen = computed(() => new Set(selected.value))

function toggle(scope: string) {
  const next = new Set(selected.value)
  if (next.has(scope)) next.delete(scope)
  else next.add(scope)
  selected.value = [...next]
}

function applyPreset(scopes: string[]) {
  selected.value = [...scopes]
}

function matchesPreset(scopes: string[]): boolean {
  if (scopes.length !== selected.value.length) return false
  return scopes.every((scope) => chosen.value.has(scope))
}

/** `admin` subsumes the rest, so ticking anything else alongside says nothing. */
const adminChosen = computed(() => chosen.value.has('admin'))
</script>

<template>
  <div class="flex flex-col gap-3">
    <div class="flex flex-wrap items-center gap-1.5">
      <span class="cq-label">Preset</span>
      <UButton
        v-for="preset in PRESETS"
        :key="preset.label"
        :variant="matchesPreset(preset.scopes) ? 'subtle' : 'ghost'"
        color="neutral"
        size="xs"
        :title="preset.hint"
        @click="applyPreset(preset.scopes)"
      >
        {{ preset.label }}
      </UButton>
      <UButton
        v-if="selected.length"
        icon="i-lucide-x"
        color="neutral"
        variant="ghost"
        size="xs"
        aria-label="Clear the selected scopes"
        @click="selected = []"
      />
      <span class="cq-num ml-auto text-xs text-muted">{{ selected.length }} selected</span>
    </div>

    <p
      v-if="adminChosen"
      class="rounded-md border border-warning/40 bg-warning/5 px-3 py-2 text-xs text-warning"
    >
      <code>admin</code> already grants everything below. The individual scopes
      add nothing while it is set.
    </p>

    <div class="flex max-h-64 flex-col gap-3 overflow-y-auto rounded-lg border border-default p-3">
      <div
        v-for="group in SCOPE_GROUPS"
        :key="group.label"
      >
        <p class="cq-label mb-1.5">
          {{ group.label }}
        </p>
        <div class="flex flex-col gap-1">
          <UCheckbox
            v-for="scope in group.scopes"
            :key="scope.value"
            :model-value="chosen.has(scope.value)"
            :ui="{ label: 'font-mono text-xs' }"
            :label="scope.value"
            :description="scope.hint"
            @update:model-value="() => toggle(scope.value)"
          />
        </div>
      </div>
    </div>
  </div>
</template>
