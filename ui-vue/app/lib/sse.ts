/**
 * One server-sent-events client for the dashboard's two streams.
 *
 * Carried over from the React tree unchanged (ADR-0004 names it as portable).
 * There, the runners feed and the log console had each grown their own `fetch` + `ReadableStream` reader, with the same frame
 * assembly written twice (#585). Chunk-boundary handling is the part that
 * matters: a single SSE event can arrive split across two reads, and getting it
 * wrong loses events under load rather than failing loudly — the failure nobody
 * notices until a runner goes quiet.
 *
 * Deliberately framework-free. Nothing here imports React or the auth store;
 * the token and the refresh are passed in. That keeps the tricky half testable
 * without a DOM, and it is the half worth carrying forward unchanged if the
 * dashboard is ever ported (#589) — which is what this copy is.
 *
 * `EventSource` is not an option: it cannot send an `Authorization` header,
 * and these streams are Bearer-authenticated.
 */

/**
 * Assembles `data` payloads out of a byte stream.
 *
 * Separate from the transport because it is the part with edge cases, and
 * because those edge cases are cheap to test and expensive to get wrong.
 */
export class SseFrameParser {
  private buf = ''

  /**
   * Feed a decoded chunk; get back every complete event's `data` payload.
   *
   * Handles what the two hand-rolled versions did not:
   *
   * * **CRLF.** The SSE grammar allows `\r\n` and a lone `\r` as line
   *   terminators. Both call sites split on `'\n\n'` only, so a server or
   *   proxy that normalised to CRLF would have produced one endless frame.
   * * **Multi-line `data:`.** The spec concatenates repeated `data:` lines
   *   with a newline. Both versions took `lines.find(startsWith('data:'))` —
   *   the *first* one — and silently dropped the rest. Harmless today, since
   *   the server sends JSON and `JSON.stringify` escapes newlines, but it is
   *   a trap laid for whoever changes the payload.
   * * **Comment lines.** A line starting with `:` is a keepalive and carries
   *   no data. Skipped rather than parsed into a dropped frame.
   */
  push(chunk: string): string[] {
    this.buf += chunk.replace(/\r\n|\r/g, '\n')
    const frames = this.buf.split('\n\n')
    // The last element is either an empty string (the chunk ended on a frame
    // boundary) or a partial frame. Either way it stays in the buffer.
    this.buf = frames.pop() ?? ''

    const payloads: string[] = []
    for (const frame of frames) {
      const data: string[] = []
      for (const line of frame.split('\n')) {
        if (line.startsWith(':')) continue
        if (!line.startsWith('data:')) continue
        // One optional space after the colon belongs to the framing, not the
        // payload.
        const value = line.slice(5)
        data.push(value.startsWith(' ') ? value.slice(1) : value)
      }
      if (data.length) payloads.push(data.join('\n'))
    }
    return payloads
  }
}

export interface SseStreamOptions {
  /**
   * The URL to open. Called per attempt with whether a previous attempt ever
   * reached an open stream — the console uses that to stop asking for a
   * backfill snapshot on reconnects, so a dropped stream does not replay
   * events already in the buffer.
   */
  url: (hasOpened: boolean) => string
  /** Current access token, read fresh per attempt — it may have been renewed. */
  getToken: () => string | null | undefined
  /**
   * Called on a 401. Resolve `true` to retry with a new token, `false` when
   * the session is genuinely gone (in which case the stream ends).
   */
  refresh: () => Promise<boolean>
  /** One event's `data` payload, already assembled across chunk boundaries. */
  onData: (payload: string) => void
  onOpen?: () => void
  onClose?: () => void
  /**
   * Statuses that end the stream permanently instead of reconnecting.
   *
   * The console passes 403 (the feed is admin-only, so the answer will not
   * change) and 503 (an older binary with no console hub). Without this a
   * non-admin session reconnect-loops against a settled answer.
   */
  fatalStatuses?: readonly number[]
  /** Called once when a fatal status arrives, before the stream ends. */
  onFatal?: (status: number) => void
  /** Delay before attempt `n` (0-based). Default: 1s doubling to 30s. */
  backoff?: (attempt: number) => number
}

const defaultBackoff = (attempt: number) => Math.min(1000 * 2 ** attempt, 30_000)

/**
 * Open an SSE stream and keep it open, reconnecting with backoff.
 *
 * Returns a function that closes it. Safe to call more than once.
 */
export function createSseStream(options: SseStreamOptions): () => void {
  const {
    url,
    getToken,
    refresh,
    onData,
    onOpen,
    onClose,
    fatalStatuses = [],
    onFatal,
    backoff = defaultBackoff,
  } = options

  let stopped = false
  let controller = new AbortController()
  let timer: ReturnType<typeof setTimeout> | undefined
  let attempt = 0
  let hasOpened = false

  const stop = () => {
    stopped = true
    controller.abort()
    clearTimeout(timer)
  }

  async function connect(): Promise<void> {
    let reconnect = true
    try {
      const token = getToken()
      const response = await fetch(url(hasOpened), {
        signal: controller.signal,
        headers: {
          Accept: 'text/event-stream',
          ...(token ? { Authorization: `Bearer ${token}` } : {}),
        },
      })

      if (response.status === 401) {
        // The stream outlives the one-hour access token by design, so a 401
        // here is routine. Refresh and let the backoff reconnect; a genuinely
        // dead session fails the refresh, which clears it (#454).
        if (!(await refresh())) reconnect = false
        return
      }
      if (fatalStatuses.includes(response.status)) {
        reconnect = false
        onFatal?.(response.status)
        return
      }
      if (!response.ok || !response.body) throw new Error(`SSE ${response.status}`)

      hasOpened = true
      attempt = 0
      onOpen?.()

      const reader = response.body.getReader()
      const decoder = new TextDecoder()
      const parser = new SseFrameParser()
      for (;;) {
        const { done, value } = await reader.read()
        if (done) break
        for (const payload of parser.push(decoder.decode(value, { stream: true }))) {
          onData(payload)
        }
      }
    } catch {
      // Network error, abort, or a non-OK status — reconnect below.
    } finally {
      onClose?.()
    }

    if (stopped || !reconnect) return
    const delay = backoff(attempt)
    attempt++
    timer = setTimeout(() => {
      if (stopped) return
      controller = new AbortController()
      void connect()
    }, delay)
  }

  void connect()
  return stop
}
