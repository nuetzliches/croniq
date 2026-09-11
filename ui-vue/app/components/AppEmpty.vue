<script setup lang="ts">
/**
 * An empty list, said properly.
 *
 * A fresh install is *all* empty states, so these carry more of the first
 * impression than any populated screen does. The shipping dashboard's version
 * is an icon and two lines centred in whatever space is left over — which on
 * its Executions page means a 60%-wide pane containing nothing but an
 * apology (docs/ui-visual-design.md).
 *
 * The difference here is `action`: an empty state that only reports emptiness
 * wastes the one moment the user is definitely looking for a way forward. Most
 * of the lists in this app have an obvious next step — create a job, connect a
 * runner, trigger a run — and the slot exists so nobody has to invent a layout
 * for it later.
 */
withDefaults(
  defineProps<{
    /** What is empty, as a statement: "No runs yet". */
    title: string
    /** Why, or what would change it. One sentence. */
    description?: string
    icon?: string
    /** Vertical breathing room. `tight` for an empty panel inside a page. */
    size?: 'default' | 'tight'
  }>(),
  { description: undefined, icon: 'i-lucide-inbox', size: 'default' },
)
</script>

<template>
  <div
    class="flex flex-col items-center text-center"
    :class="size === 'tight' ? 'gap-2 py-8' : 'gap-3 py-16'"
  >
    <UIcon
      :name="icon"
      class="text-dimmed"
      :class="size === 'tight' ? 'size-6' : 'size-8'"
      aria-hidden="true"
    />
    <div>
      <p class="font-medium text-highlighted">
        {{ title }}
      </p>
      <p
        v-if="description"
        class="mt-1 max-w-sm text-sm text-muted"
      >
        {{ description }}
      </p>
    </div>
    <!-- The way forward, when there is one. -->
    <div
      v-if="$slots.action"
      class="mt-1"
    >
      <slot name="action" />
    </div>
  </div>
</template>
