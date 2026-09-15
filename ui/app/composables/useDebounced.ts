import { onScopeDispose, ref, watch, type Ref } from 'vue'

/**
 * A value that trails its source, so typing does not become a request per
 * keystroke.
 *
 * The text filters on Runs, Audit and Alert deliveries feed straight into a
 * query key. Typing an eight-character job key was eight cache entries and
 * eight `limit=200` requests, seven of them answered and thrown away — and on
 * a busy server each one is a 200-row scan (issue #730).
 *
 * Only the *query* trails. The input stays on the immediate value and the URL
 * keeps being written on every keystroke: filter state lives in the URL here,
 * a pasted link is the point, and `router.replace` does not grow the back
 * button, so there is nothing to protect history from.
 *
 * @param source a getter, so a `computed` or a route read can be passed
 *   directly rather than unwrapped first.
 * @param options.delayMs how long the source must hold still. 250ms is below
 *   the threshold where a pause reads as the interface thinking, and above a
 *   fast typist's gaps.
 * @param options.atOnce values that skip the wait. Clearing is the one case
 *   where the delay is felt as lag rather than as nothing: emptying the box or
 *   pressing "Clear filters" is deliberate, with an obvious expected result,
 *   and a quarter second of the narrower list still on screen reads as a
 *   control that did not work.
 */
export function useDebounced<T>(
  source: () => T,
  options: { delayMs?: number; atOnce?: (value: T) => boolean } = {},
): Ref<T> {
  const { delayMs = 250, atOnce = (value: T) => !value } = options
  const settled = ref(source()) as Ref<T>
  let timer: ReturnType<typeof setTimeout> | undefined

  function cancel() {
    if (timer !== undefined) clearTimeout(timer)
    timer = undefined
  }

  watch(source, (next) => {
    // A newer keystroke always supersedes the pending one, so the wait is from
    // the last character rather than the first.
    cancel()
    if (atOnce(next)) {
      settled.value = next
      return
    }
    timer = setTimeout(() => {
      timer = undefined
      settled.value = next
    }, delayMs)
  })

  // A pending timer holds a closure over the screen's state; on a view
  // navigated away from mid-keystroke it would fire into a disposed scope.
  onScopeDispose(cancel)

  return settled
}
