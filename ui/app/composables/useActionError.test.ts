import { describe, expect, it } from 'vitest'
import { ApiError } from '~/api/client'
import { describeRefusal, useActionError } from './useActionError'

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

/**
 * The reading on its own, for the `try` blocks that also produce a value.
 *
 * Fourteen copies of it lived in nine components. Every copy chained `??`,
 * which does not fall through an empty string — so a refusal whose body said
 * `{"message": ""}` rendered an empty alert instead of the fallback (#729).
 */
describe('describeRefusal', () => {
  it('prefers the body over the status line', () => {
    const caught = new ApiError(422, 'Request failed with 422', {
      message: 'Line 3: unknown weekday "Funday".',
    })

    expect(describeRefusal(caught)).toBe('Line 3: unknown weekday "Funday".')
  })

  it('treats a blank body message as absent', () => {
    // The copies this replaced would have shown an empty alert here.
    const caught = new ApiError(500, 'Internal Server Error', { message: '   ' })

    expect(describeRefusal(caught)).toBe('Internal Server Error')
  })

  it('takes the caller’s own fallback', () => {
    // Enrolment and replay say what was refused, not just that something was.
    expect(describeRefusal(new ApiError(500, ''), 'Replay was refused.')).toBe(
      'Replay was refused.',
    )
  })

  it('survives a thrown non-error', () => {
    expect(describeRefusal('a bare string')).toBe('The server refused that.')
  })
})
