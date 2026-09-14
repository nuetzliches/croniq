<script setup lang="ts">
import { computed } from 'vue'
import { ApiError } from '~/api/client'

/**
 * A request that failed, said usefully.
 *
 * Two things this does that a bare "something went wrong" does not.
 *
 * It distinguishes *kinds*: a 403 is a permanent answer and a retry button on
 * it is a lie, while a network failure is usually worth one more try. Offering
 * "Retry" indiscriminately teaches people to click it on errors that will
 * never change.
 *
 * And it keeps the server's own message. Croniq's API says useful things —
 * which scope was missing, which field was rejected — and replacing that with
 * a friendly generic is the most common way a dashboard makes its own backend
 * harder to operate.
 */
const props = defineProps<{
  error: unknown
  /** Omitted when the caller has nothing sensible to retry. */
  onRetry?: () => void
}>()

const status = computed(() =>
  props.error instanceof ApiError ? props.error.status : undefined,
)

const title = computed(() => {
  switch (status.value) {
    case 401:
      return 'Session expired'
    case 403:
      return 'Not allowed'
    case 404:
      return 'Not found'
    case 503:
      return 'Server unavailable'
    default:
      return status.value && status.value >= 500 ? 'Server error' : 'Could not load'
  }
})

const detail = computed(() => {
  const message = (props.error as Error | undefined)?.message
  if (status.value === 403) {
    return message ?? 'This account does not have the scope this view needs.'
  }
  if (status.value === undefined) {
    return message ?? 'The server could not be reached.'
  }
  return message
})

/**
 * 403 and 404 are settled answers; retrying them produces the same result and
 * a button that promises otherwise is worse than no button.
 */
const retryable = computed(
  () => props.onRetry !== undefined && status.value !== 403 && status.value !== 404,
)
</script>

<template>
  <div class="flex flex-col items-center gap-3 py-12 text-center">
    <UIcon
      name="i-lucide-triangle-alert"
      class="size-7 text-error"
      aria-hidden="true"
    />
    <div>
      <p class="font-medium text-highlighted">
        {{ title }}
      </p>
      <p
        v-if="detail"
        class="mt-1 max-w-md text-sm text-muted"
      >
        {{ detail }}
      </p>
    </div>
    <UButton
      v-if="retryable"
      variant="subtle"
      color="neutral"
      icon="i-lucide-rotate-cw"
      size="sm"
      @click="onRetry?.()"
    >
      Try again
    </UButton>
  </div>
</template>
