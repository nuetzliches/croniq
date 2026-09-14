<script setup lang="ts">
import { ref } from 'vue'

/**
 * A value the server will never show again.
 *
 * API keys, personal access tokens, invitation links and TOTP recovery codes
 * all share one property that the surrounding UI keeps forgetting: this render
 * is the only copy. Every one of them was a separate piece of markup in the
 * React tree, and the recovery codes were the one that got dropped — the
 * checkbox asking the user to confirm they had saved them was rendered above
 * nothing at all.
 *
 * So it is one component, and it is deliberately hard to dismiss by accident:
 * the close action is disabled until the value has been copied or the
 * acknowledgement ticked.
 */
const props = defineProps<{
  title: string
  /** Why it cannot be recovered — the specific reason, not a generic warning. */
  description: string
  /** One line, or several (recovery codes). */
  value: string | string[]
  /** Wording for the acknowledgement, when the default does not fit. */
  acknowledgement?: string
}>()

const emit = defineEmits<{ done: [] }>()

const copied = ref(false)
const acknowledged = ref(false)

const text = () => (Array.isArray(props.value) ? props.value.join('\n') : props.value)

async function copy() {
  try {
    await navigator.clipboard.writeText(text())
    copied.value = true
  } catch {
    // Clipboard access can be refused; the value is on screen and selectable,
    // and the acknowledgement is the other way out.
  }
}
</script>

<template>
  <div
    class="flex flex-col gap-3 rounded-lg border border-warning/40 bg-warning/5 p-4"
    role="alert"
  >
    <div class="flex items-start gap-2">
      <UIcon
        name="i-lucide-key-round"
        class="mt-0.5 size-4 shrink-0 text-warning"
      />
      <div class="min-w-0">
        <p class="text-sm font-medium">
          {{ title }}
        </p>
        <p class="mt-0.5 text-xs text-muted">
          {{ description }}
        </p>
      </div>
    </div>

    <pre class="overflow-x-auto rounded-md border border-default bg-default p-3 font-mono text-xs select-all">{{ Array.isArray(value) ? value.join('\n') : value }}</pre>

    <div class="flex flex-wrap items-center gap-3">
      <UButton
        :icon="copied ? 'i-lucide-check' : 'i-lucide-copy'"
        color="neutral"
        variant="subtle"
        size="xs"
        @click="copy"
      >
        {{ copied ? 'Copied' : 'Copy' }}
      </UButton>
      <UCheckbox
        v-model="acknowledged"
        :label="acknowledgement ?? 'I have stored this somewhere safe'"
      />
      <UButton
        class="ml-auto"
        size="xs"
        :disabled="!copied && !acknowledged"
        :title="
          !copied && !acknowledged
            ? 'Copy the value or confirm you have stored it — it cannot be shown again'
            : undefined
        "
        @click="emit('done')"
      >
        Done
      </UButton>
    </div>
  </div>
</template>
