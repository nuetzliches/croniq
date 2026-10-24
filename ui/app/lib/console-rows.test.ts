import { describe, expect, it, vi } from 'vitest'
import type { LogEvent } from '~/composables/useConsoleStream'
import { createConsoleRows, fieldText } from './console-rows'

let seq = 0
function event(level: string, message: string, fields: Record<string, unknown> = {}): LogEvent {
  seq += 1
  return { ts: '2026-10-24T12:00:00.000Z', level, target: 'croniq_server', message, fields, seq }
}

const ALL = new Set(['debug', 'info', 'warn', 'error'])

describe('createConsoleRows', () => {
  it('derives each row once per event, not once per frame', () => {
    // Review 2026-10-24, U-4: every frame re-formatted the whole 2 000-event
    // buffer to show one new line.
    const format = vi.fn((iso: string) => iso.slice(11, 19))
    const rows = createConsoleRows(format)
    const buffer = Array.from({ length: 50 }, (_, i) => event('info', `line ${i}`))

    rows.filter(buffer, ALL, '')
    expect(format).toHaveBeenCalledTimes(50)

    // The next frame: same events plus one newcomer.
    const next = [...buffer, event('warn', 'newcomer')]
    const second = rows.filter(next, ALL, '')
    expect(format).toHaveBeenCalledTimes(51)
    expect(second).toHaveLength(51)
    expect(second[0]).toBe(rows.row(buffer[0]!))
  })

  it('filters by level and by message or target, case-insensitively', () => {
    const rows = createConsoleRows((iso) => iso)
    const list = [
      event('debug', 'Noise'),
      event('info', 'Scheduler tick'),
      event('error', 'Dispatch failed'),
    ]

    expect(rows.filter(list, new Set(['info', 'error']), '').map((r) => r.event.message)).toEqual([
      'Scheduler tick',
      'Dispatch failed',
    ])
    expect(rows.filter(list, ALL, '  DISPATCH ').map((r) => r.event.message)).toEqual([
      'Dispatch failed',
    ])
    expect(rows.filter(list, ALL, 'croniq_server')).toHaveLength(3)
  })

  it('keeps the gutter and field text the template shows', () => {
    const rows = createConsoleRows((iso) => iso)
    const [warn, error, info] = rows.filter(
      [event('warn', 'w'), event('error', 'e', { job: 'a:b', n: 2 }), event('info', 'i')],
      ALL,
      '',
    )
    expect(warn!.gutter).toBe('border-warning')
    expect(error!.gutter).toBe('border-error')
    expect(info!.gutter).toBe('border-transparent')
    expect(error!.fields).toBe('job=a:b n=2')
  })
})

describe('fieldText', () => {
  it('serialises non-string values and leaves strings bare', () => {
    expect(fieldText(event('info', 'x', { a: 'b', c: { d: 1 } }))).toBe('a=b c={"d":1}')
    expect(fieldText(event('info', 'x'))).toBe('')
  })
})
