<script setup lang="ts">
import { computed } from 'vue'
import { useRoute } from 'vue-router'

/**
 * A screen the rebuild has not reached yet.
 *
 * Named for what it is. During the parallel phase the shell has to be walkable
 * to be judged at all, but a blank route reads as a broken page rather than an
 * unbuilt one — so this says which build step owns it and points at the
 * dashboard that does have it today.
 */
const route = useRoute()
const title = computed(() => String(route.meta.title ?? 'This screen'))
const step = computed(() => route.meta.step as number | undefined)
</script>

<template>
  <div class="mx-auto max-w-lg py-16 text-center">
    <UIcon
      name="i-lucide-hard-hat"
      class="mx-auto mb-3 size-8 text-muted"
    />
    <h2 class="mb-1 text-lg font-medium">
      {{ title }} is not built yet
    </h2>
    <p class="text-sm text-muted">
      Part of
      <template v-if="step">
        step {{ step }}
      </template>
      <template v-else>
        a later step
      </template>
      of the Vue rebuild — see
      <code class="font-mono">docs/ui-screen-inventory.md</code>. The shipping
      dashboard still has it.
    </p>
  </div>
</template>
