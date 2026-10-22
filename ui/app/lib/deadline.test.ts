import { describe, expect, it } from 'vitest'
import { deadlineFrom } from './deadline'

describe('deadlineFrom', () => {
  const now = Date.parse('2026-10-22T10:00:00Z')

  it('adds minutes, hours and days', () => {
    expect(deadlineFrom('30m', now)).toBe('2026-10-22T10:30:00.000Z')
    expect(deadlineFrom('2h', now)).toBe('2026-10-22T12:00:00.000Z')
    expect(deadlineFrom('3d', now)).toBe('2026-10-25T10:00:00.000Z')
  })

  it('takes spaces and capitals', () => {
    expect(deadlineFrom(' 1 H ', now)).toBe('2026-10-22T11:00:00.000Z')
  })

  it('refuses anything else', () => {
    for (const input of ['', '1w', 'soon', '1.5h', '-1h', '30']) {
      expect(deadlineFrom(input, now)).toBeNull()
    }
  })
})
