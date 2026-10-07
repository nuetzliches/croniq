<script setup lang="ts">
import { computed } from 'vue'
import { useFavorites } from '~/composables/useFavorites'

/**
 * The star beside a job: one click to keep an eye on it, one to stop.
 *
 * Renders nothing until the user's favorites have loaded, and nothing at all
 * for a session with no user behind it — see `useFavorites`.
 *
 * Lucide has no filled star; a starred one is the outline with its path
 * filled. The `fill="none"` sits on the path, so that is what the class targets.
 */
const props = defineProps<{ jobKey: string }>()

const { available, isFavorite, toggle, error } = useFavorites()
const on = computed(() => isFavorite(props.jobKey))
const label = computed(() => (on.value ? 'Remove from favorites' : 'Add to favorites'))
</script>

<template>
  <UButton
    v-if="available"
    icon="i-lucide-star"
    :color="error ? 'error' : on ? 'warning' : 'neutral'"
    variant="ghost"
    size="xs"
    :aria-pressed="on"
    :aria-label="label"
    :title="error ?? label"
    :ui="{ leadingIcon: on ? '[&_path]:fill-current' : 'opacity-60' }"
    data-testid="favorite-toggle"
    @click.stop.prevent="toggle(jobKey)"
  />
</template>
