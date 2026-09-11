<script setup lang="ts">
/**
 * Waiting, with the product's own mark rather than a generic ring.
 *
 * The shipping dashboard spins its brand mark as the loading indicator, which
 * is a small thing that makes a slow moment feel like part of the product
 * instead of part of the framework. Carried over.
 *
 * `label` is not decoration: a spinner with no accessible name is invisible to
 * a screen reader, so the wait is announced as nothing at all.
 */
withDefaults(
  defineProps<{
    label?: string
    size?: 'default' | 'tight'
  }>(),
  { label: 'Loading', size: 'default' },
)
</script>

<template>
  <div
    class="flex flex-col items-center justify-center gap-3"
    :class="size === 'tight' ? 'py-8' : 'py-16'"
    role="status"
    :aria-label="label"
  >
    <BrandMark
      :size="size === 'tight' ? 20 : 28"
      spinning
      class="text-primary"
    />
    <span class="sr-only">{{ label }}</span>
  </div>
</template>
