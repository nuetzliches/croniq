import { ref } from 'vue'
import { ApiError } from '~/api/client'

/** First of these that actually says something. */
function firstNonEmpty(...candidates: Array<string | undefined>): string | undefined {
  return candidates.find((candidate) => candidate !== undefined && candidate.trim() !== '')
}

/**
 * Run a mutation and keep whatever the server said when it refused.
 *
 * Every destructive control in this dashboard needs the same three things: run
 * it, clear the previous complaint, and put the new one somewhere the operator
 * can see. Screens that wrote this out by hand got it right; the ones that
 * called `mutateAsync` bare did not, and a 403, a 409 or a 5xx there was
 * indistinguishable from success-but-slow — the popover stayed open, the row
 * stayed put, and nothing said why (issue #664).
 *
 * Vue routes a rejection out of an `@click` handler to its own error handler,
 * which by default only writes to the console. So "unhandled" here means
 * "invisible to the person who pressed the button", not "crashes the page".
 *
 * The server's own wording is preferred over anything invented here: a
 * `replay_max_age` refusal or a "runner still holds claims" conflict explains
 * itself far better than "something went wrong" ever could.
 */
export function useActionError() {
  const error = ref<string | null>(null)

  /** Clear the message, e.g. when a dialog reopens. */
  function clear() {
    error.value = null
  }

  /**
   * Await `fn`, capturing a refusal instead of letting it escape.
   *
   * Returns whether it succeeded, so a caller can decide what to do next —
   * closing a popover on success but leaving it open on failure, say, where
   * closing it would hide the message that just appeared.
   */
  async function attempt(fn: () => Promise<unknown>): Promise<boolean> {
    error.value = null
    try {
      await fn()
      return true
    } catch (caught) {
      const body = caught instanceof ApiError ? (caught.body as { message?: string }) : undefined
      // `??` alone is not enough: an empty string is neither null nor
      // undefined, so a refusal with a blank message would render an empty
      // alert — visibly broken rather than merely unhelpful. The copies of
      // this block scattered through the components all have that edge.
      error.value =
        firstNonEmpty(body?.message, (caught as Error | undefined)?.message) ??
        'The server refused that.'
      return false
    }
  }

  return { error, attempt, clear }
}
