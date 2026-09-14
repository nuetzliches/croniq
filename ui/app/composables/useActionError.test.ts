import { describe, expect, it } from 'vitest'
import { ApiError } from '~/api/client'
import { useActionError } from './useActionError'

/**
 * What an operator is told when a destructive control is refused.
 *
 * The four sites this was extracted for called `mutateAsync` bare, so a 403, a
 * 409 or a 5xx reached Vue's default error handler and went to the console —
 * invisible to the person who pressed the button, and indistinguishable from
 * success-but-slow (issue #664).
 */
describe('useActionError', () => {
  it('reports the server’s own words', async () => {
    // The whole reason for preferring the body: "runner still holds claims"
    // tells an operator what to do next; "Request failed with 409" does not.
    const { error, attempt } = useActionError()

    const ok = await attempt(() =>
      Promise.reject(
        new ApiError(409, 'Request failed with 409', {
          error: 'runner_busy',
          message: 'That runner still holds 3 claimed executions.',
        }),
      ),
    )

    expect(ok).toBe(false)
    expect(error.value).toBe('That runner still holds 3 claimed executions.')
  })

  it('falls back to the error message when there is no body', async () => {
    const { error, attempt } = useActionError()

    await attempt(() => Promise.reject(new Error('Failed to fetch')))

    expect(error.value).toBe('Failed to fetch')
  })

  it('reports something rather than nothing for a thrown non-error', async () => {
    const { error, attempt } = useActionError()

    await attempt(() => Promise.reject(new ApiError(500, '')))

    expect(error.value).toBe('The server refused that.')
  })

  it('clears the previous message before each attempt', async () => {
    // Otherwise a retry that succeeds leaves the last complaint on screen,
    // which reads as "it failed again".
    const { error, attempt } = useActionError()

    await attempt(() => Promise.reject(new Error('first')))
    expect(error.value).toBe('first')

    const ok = await attempt(() => Promise.resolve('fine'))

    expect(ok).toBe(true)
    expect(error.value).toBeNull()
  })

  it('clears on demand, for a dialog that reopens', async () => {
    const { error, attempt, clear } = useActionError()

    await attempt(() => Promise.reject(new Error('stale complaint')))
    clear()

    expect(error.value).toBeNull()
  })
})
