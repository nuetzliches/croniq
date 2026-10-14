import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createSseStream, SseFrameParser } from './sse'

/**
 * The frame assembly, which is the reason #585 exists. Everything below was
 * either handled identically by the two hand-rolled clients (and is asserted
 * here so a refactor cannot quietly drop it) or handled by neither.
 */
describe('SseFrameParser', () => {
  it('returns nothing until a frame is terminated', () => {
    const parser = new SseFrameParser()
    expect(parser.push('data: {"a":1}')).toEqual([])
    expect(parser.push('\n\n')).toEqual(['{"a":1}'])
  })

  it('reads several frames out of one chunk', () => {
    const parser = new SseFrameParser()
    expect(parser.push('data: one\n\ndata: two\n\n')).toEqual(['one', 'two'])
  })

  /**
   * The case the whole class exists for: a single event split across two
   * reads. Getting this wrong loses events under load rather than failing.
   */
  it('reassembles an event split across chunk boundaries', () => {
    const parser = new SseFrameParser()
    expect(parser.push('data: {"runner":"a')).toEqual([])
    expect(parser.push('lpha"}\n\n')).toEqual(['{"runner":"alpha"}'])
  })

  it('reassembles an event split inside the frame terminator', () => {
    const parser = new SseFrameParser()
    expect(parser.push('data: x\n')).toEqual([])
    expect(parser.push('\ndata: y\n\n')).toEqual(['x', 'y'])
  })

  it('keeps a partial frame buffered across many small chunks', () => {
    const parser = new SseFrameParser()
    for (const char of 'data: hello') expect(parser.push(char)).toEqual([])
    expect(parser.push('\n\n')).toEqual(['hello'])
  })

  it('strips exactly one space after the colon', () => {
    const parser = new SseFrameParser()
    expect(parser.push('data:  two-spaces\n\n')).toEqual([' two-spaces'])
  })

  it('accepts a data line with no space at all', () => {
    const parser = new SseFrameParser()
    expect(parser.push('data:tight\n\n')).toEqual(['tight'])
  })

  /**
   * Not handled by either original: they took the *first* `data:` line and
   * dropped the rest. Safe today because the payloads are JSON, which escapes
   * newlines — a trap for whoever changes that.
   */
  it('joins repeated data lines with a newline', () => {
    const parser = new SseFrameParser()
    expect(parser.push('data: first\ndata: second\n\n')).toEqual(['first\nsecond'])
  })

  /**
   * Also not handled by either: both split on '\n\n' only, so a CRLF stream
   * would have produced one frame that never terminated.
   */
  it('accepts CRLF line endings', () => {
    const parser = new SseFrameParser()
    expect(parser.push('data: crlf\r\n\r\n')).toEqual(['crlf'])
  })

  it('accepts a bare CR as a line terminator', () => {
    const parser = new SseFrameParser()
    expect(parser.push('data: cr\r\r')).toEqual(['cr'])
  })

  it('skips comment lines without emitting an empty payload', () => {
    const parser = new SseFrameParser()
    expect(parser.push(': keepalive\n\n')).toEqual([])
    expect(parser.push('data: after\n\n')).toEqual(['after'])
  })

  it('ignores non-data fields', () => {
    const parser = new SseFrameParser()
    expect(parser.push('event: ping\nid: 7\ndata: payload\n\n')).toEqual(['payload'])
  })

  it('emits an empty payload for an empty data line', () => {
    // `data:` with nothing after it is a legitimate empty payload, distinct
    // from a frame that carries no data field at all.
    const parser = new SseFrameParser()
    expect(parser.push('data:\n\n')).toEqual([''])
  })

  it('does not re-emit a frame on the next push', () => {
    const parser = new SseFrameParser()
    expect(parser.push('data: once\n\n')).toEqual(['once'])
    expect(parser.push('data: twice\n\n')).toEqual(['twice'])
  })
})

/**
 * The transport around the parser: which exits reconnect and which end the
 * stream. Until the 401 case below, nothing here was tested, and a `return`
 * inside the `try` had quietly stopped a refreshed stream from ever reopening.
 */
describe('createSseStream', () => {
  const encoder = new TextEncoder()

  /** A stream that delivers `chunks` and then ends, as a dropped connection does. */
  const stream = (...chunks: string[]) =>
    new Response(
      new ReadableStream({
        start(controller) {
          for (const chunk of chunks) controller.enqueue(encoder.encode(chunk))
          controller.close()
        },
      }),
      { status: 200 },
    )
  const status = (code: number) => new Response(null, { status: code })

  beforeEach(() => vi.useFakeTimers())
  afterEach(() => {
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  it('reconnects with the renewed token after a 401 whose refresh succeeded', async () => {
    // The stream outlives the one-hour access token, so the first request after
    // a drop is the one that finds it expired. The dashboard then showed the
    // last frame behind a grey dot until it was reloaded.
    let token = 'stale'
    const sent: (string | null)[] = []
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: string, init: RequestInit) => {
        sent.push(new Headers(init.headers).get('Authorization'))
        return sent.length === 1 ? status(401) : stream('data: hello\n\n')
      }),
    )
    const data: string[] = []
    const stop = createSseStream({
      url: () => '/v1/stream',
      getToken: () => token,
      refresh: async () => {
        token = 'fresh'
        return true
      },
      onData: (payload) => data.push(payload),
    })

    await vi.advanceTimersByTimeAsync(1000)
    expect(sent).toEqual(['Bearer stale', 'Bearer fresh'])
    expect(data).toEqual(['hello'])
    stop()
  })

  it('ends for good when the refresh says the session is gone', async () => {
    const fetchMock = vi.fn(async () => status(401))
    vi.stubGlobal('fetch', fetchMock)
    const onClose = vi.fn()
    createSseStream({
      url: () => '/v1/stream',
      getToken: () => 'stale',
      refresh: async () => false,
      onData: () => {},
      onClose,
    })

    await vi.advanceTimersByTimeAsync(60_000)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('backs off instead of hammering a server that keeps answering 401', async () => {
    const fetchMock = vi.fn(async () => status(401))
    vi.stubGlobal('fetch', fetchMock)
    const stop = createSseStream({
      url: () => '/v1/stream',
      getToken: () => 'token',
      refresh: async () => true,
      onData: () => {},
    })

    // Default backoff: 1 s, then 2 s — attempts at 0, 1 s and 3 s.
    await vi.advanceTimersByTimeAsync(999)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(1)
    expect(fetchMock).toHaveBeenCalledTimes(2)
    await vi.advanceTimersByTimeAsync(1999)
    expect(fetchMock).toHaveBeenCalledTimes(2)
    await vi.advanceTimersByTimeAsync(1)
    expect(fetchMock).toHaveBeenCalledTimes(3)
    stop()
  })

  it('ends on a fatal status and reports it', async () => {
    const fetchMock = vi.fn(async () => status(403))
    vi.stubGlobal('fetch', fetchMock)
    const onFatal = vi.fn()
    createSseStream({
      url: () => '/v1/stream',
      getToken: () => 'token',
      refresh: async () => true,
      fatalStatuses: [403],
      onFatal,
      onData: () => {},
    })

    await vi.advanceTimersByTimeAsync(60_000)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(onFatal).toHaveBeenCalledWith(403)
  })

  it('reconnects after the stream ends, and says it has been open before', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => stream('data: one\n\n')),
    )
    const urls: boolean[] = []
    const data: string[] = []
    const stop = createSseStream({
      url: (hasOpened) => {
        urls.push(hasOpened)
        return '/v1/stream'
      },
      getToken: () => 'token',
      refresh: async () => true,
      onData: (payload) => data.push(payload),
    })

    await vi.advanceTimersByTimeAsync(1000)
    expect(urls).toEqual([false, true])
    expect(data).toEqual(['one', 'one'])
    stop()
  })
})
