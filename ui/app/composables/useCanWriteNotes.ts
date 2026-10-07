import { computed, type ComputedRef } from 'vue'
import { useCurrentUser } from '~/api/queries'

/**
 * May this session write notes? Operators and admins hold `notes:write`;
 * viewers read only.
 *
 * Without a user record — an API-key session, or before the query answers —
 * the answer is `true`, for the reason `useIsAdmin` gives: the server enforces
 * scopes on every request, and hiding the control from a caller entitled to it
 * is the worse mistake. Presentation only.
 */
export function useCanWriteNotes(): ComputedRef<boolean> {
  const { data: me } = useCurrentUser()
  return computed(() => (me.value ? me.value.role !== 'viewer' : true))
}
