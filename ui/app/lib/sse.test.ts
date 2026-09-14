import { describe, expect, it } from 'vitest'
import { SseFrameParser } from './sse'

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
