<script setup lang="ts">
/**
 * The KPI shape carried over from the shipping dashboard: uppercase
 * micro-label, a big number, an explaining sub-line, and room for something
 * small beside it.
 *
 * The audit rated this the most recognisable element of the existing design,
 * used identically on the dashboard and the job detail. Keeping it is the
 * cheapest way for the rebuild to still look like Croniq.
 */
withDefaults(
  defineProps<{
    label: string
    value: string | number
    sub?: string
    /** Colours the number, not the card. Use for states, not for decoration. */
    tone?: 'default' | 'success' | 'warning' | 'error'
    to?: string
  }>(),
  { sub: undefined, tone: 'default', to: undefined },
)

const toneClass = {
  default: 'text-highlighted',
  success: 'text-success',
  warning: 'text-warning',
  error: 'text-error',
}
</script>

<template>
  <component
    :is="to ? 'RouterLink' : 'div'"
    :to="to"
    class="block rounded-xl border border-default bg-default p-4 shadow-sm transition-colors"
    :class="to && 'hover:border-primary/40'"
  >
    <div class="flex items-start justify-between gap-3">
      <p class="cq-label">
        {{ label }}
      </p>
      <slot name="aside" />
    </div>
    <p
      class="cq-num mt-1.5 text-3xl font-semibold"
      :class="toneClass[tone]"
    >
      {{ value }}
    </p>
    <p
      v-if="sub"
      class="mt-0.5 truncate text-xs text-muted"
      :title="sub"
    >
      {{ sub }}
    </p>
  </component>
</template>
