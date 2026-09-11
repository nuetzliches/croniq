<script setup lang="ts">
import { computed } from 'vue'

/**
 * An execution's state, as a pill.
 *
 * Colour carries the meaning, so it is mapped once here rather than at every
 * call site — and a state the server adds later falls back to neutral rather
 * than crashing or rendering blank.
 *
 * `dead` gets error colouring but reads as its own thing: it is not a failure
 * that might still retry, it is one that has stopped trying and is waiting for
 * a person (see /dead-letters).
 *
 * It carries job lifecycle states too, and deliberately in the same mapping:
 * `active` is the same green as `completed` because both mean "nothing to do
 * here", and one pill component means a state the server adds later looks
 * consistent wherever it turns up instead of neutral in one place and
 * unstyled in another.
 */
const props = defineProps<{ state: string; size?: 'sm' | 'md' }>()

const tone = computed(() => {
  switch (props.state) {
    case 'completed':
      return { color: 'success' as const, dot: 'bg-success' }
    case 'failed':
      return { color: 'error' as const, dot: 'bg-error' }
    case 'dead':
      return { color: 'error' as const, dot: 'bg-error' }
    case 'cancelled':
      return { color: 'neutral' as const, dot: 'bg-muted' }
    case 'claimed':
      return { color: 'info' as const, dot: 'bg-info' }
    case 'queued':
      return { color: 'neutral' as const, dot: 'bg-dimmed' }
    // Job lifecycle (GET /v1/jobs/states).
    case 'active':
      return { color: 'success' as const, dot: 'bg-success' }
    case 'paused':
      return { color: 'warning' as const, dot: 'bg-warning' }
    case 'disabled':
      return { color: 'neutral' as const, dot: 'bg-muted' }
    // Not "off" — it ran out of retries and gave up, which is a fault.
    case 'exhausted':
      return { color: 'error' as const, dot: 'bg-error' }
    default:
      return { color: 'neutral' as const, dot: 'bg-dimmed' }
  }
})
</script>

<template>
  <UBadge
    :color="tone.color"
    variant="subtle"
    :size="size ?? 'sm'"
    class="gap-1.5 whitespace-nowrap"
  >
    <span
      class="size-1.5 shrink-0 rounded-full"
      :class="tone.dot"
      aria-hidden="true"
    />
    {{ state }}
  </UBadge>
</template>
