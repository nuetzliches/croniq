<script setup lang="ts">
/**
 * "Are you sure?", once, for every destructive control in the dashboard.
 *
 * Three screens had this modal copy-pasted — same `UModal`, same `#footer`,
 * same ghost Cancel beside a red action — and four other destructive controls
 * had no confirmation at all: removing a user, deleting an API client,
 * removing a runner, deleting a schedule (issue #667). Deleting an API client
 * revokes every token minted under it; removing a runner orphans its claims.
 * Both happened on one mis-click, with no undo.
 *
 * The React tree confirmed some of these through a hook that returned JSX,
 * which ADR-0004 lists among the things worth leaving behind. This is the
 * replacement it implies rather than a port of it: a component, given the
 * question and the consequence, that says nothing about who is asking.
 *
 * The consequence goes in `description`, in the caller's own words. A dialog
 * that only says "This cannot be undone" makes the reader supply the part that
 * actually matters — what will be gone, and how much of it.
 */
withDefaults(
  defineProps<{
    open: boolean
    title: string
    /** What will happen. Concrete, in the caller's words. */
    description?: string
    /** Label for the destructive action. A verb, not "OK". */
    confirmLabel?: string
    /** Bound to the action button while the mutation is in flight. */
    loading?: boolean
  }>(),
  {
    description: undefined,
    confirmLabel: 'Delete',
    loading: false,
  },
)

const emit = defineEmits<{
  'update:open': [value: boolean]
  confirm: []
}>()
</script>

<template>
  <UModal
    :open="open"
    :title="title"
    :description="description"
    @update:open="(value: boolean) => emit('update:open', value)"
  >
    <template #footer>
      <div class="flex justify-end gap-2">
        <UButton
          color="neutral"
          variant="ghost"
          @click="emit('update:open', false)"
        >
          Cancel
        </UButton>
        <UButton
          color="error"
          :loading="loading"
          @click="emit('confirm')"
        >
          {{ confirmLabel }}
        </UButton>
      </div>
    </template>
  </UModal>
</template>
