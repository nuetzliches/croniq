import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

/**
 * How arrivals reach the console buffer.
 *
 * A live tail on a busy server is the one screen in this dashboard where the
 * render cost scales with how much the *server* is doing rather than with what
 * the operator is doing. Two things made that worse than it needed to be
 * (issue #671): every arrival copied the whole 2000-element buffer and
 * re-assigned it, and the rows were keyed by array index — so once the buffer
 * was full, every arrival shifted every key and Vue re-patched all 2000 rows
 * to show one new line.
 *
 * Arrivals are batched per animation frame now, and each carries a `seq` so a
 * row can be keyed by identity. The events themselves have nothing unique
 * about them: two log lines in the same millisecond from the same target with
 * the same message are indistinguishable, and rightly so.
 */

const created: Array<{ onData: (payload: string) => void }> = []

vi.mock('~/lib/sse', () => ({
  createSseStream: (options: { onData: (payload: string) => void }) => {
    created.push(options)
    return () => undefined
  },
}))
vi.mock('~/api/session', () => ({ refreshAccessToken: vi.fn() }))

/** One frame, as the server would send it. */
function emit(message: string, level = 'info') {
  created.at(-1)!.onData(
    JSON.stringify({
      ts: '2026-09-14T12:00:00.000Z',
      level,
      target: 'scheduler',
      message,
      fields: {},
    }),
  )
}

/** Let the batch flush — a microtask here, since there is no rAF in node. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0))

describe('useConsoleStream', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    created.length = 0
    vi.resetModules()
  })

  it('gives every event a distinct, increasing seq', async () => {
    const { useConsoleStream } = await import('./useConsoleStream')
    const { events } = useConsoleStream()

    // Identical in every field the server sends: only `seq` can tell them
    // apart, which is the point.
    emit('tick')
    emit('tick')
    emit('tick')
    await settle()

    const seqs = events.value.map((event) => event.seq)
    expect(seqs).toEqual([0, 1, 2])
    expect(new Set(seqs).size).toBe(3)
  })

  it('batches arrivals instead of rebuilding the buffer per event', async () => {
    const { useConsoleStream } = await import('./useConsoleStream')
    const { events } = useConsoleStream()

    emit('one')
    emit('two')
    // Nothing has reached the view yet: that is the batch doing its job.
    expect(events.value).toHaveLength(0)

    await settle()
    expect(events.value.map((event) => event.message)).toEqual(['one', 'two'])
  })

  it('counts what falls off the end rather than losing it quietly', async () => {
    const { useConsoleStream, MAX_BUFFER } = await import('./useConsoleStream')
    const { events, dropped } = useConsoleStream()

    for (let i = 0; i < MAX_BUFFER + 5; i += 1) emit(`line ${i}`)
    await settle()

    expect(events.value).toHaveLength(MAX_BUFFER)
    expect(dropped.value).toBe(5)
    // The newest survive, the oldest go.
    expect(events.value.at(-1)!.message).toBe(`line ${MAX_BUFFER + 4}`)
    expect(events.value[0]!.message).toBe('line 5')
  })

  it('holds arrivals while paused and flushes them on resume', async () => {
    const { useConsoleStream } = await import('./useConsoleStream')
    const { events, pendingCount, togglePause } = useConsoleStream()

    togglePause()
    emit('while paused')
    await settle()

    expect(events.value).toHaveLength(0)
    expect(pendingCount.value).toBe(1)

    togglePause()
    await settle()
    expect(events.value.map((event) => event.message)).toEqual(['while paused'])
  })
})
