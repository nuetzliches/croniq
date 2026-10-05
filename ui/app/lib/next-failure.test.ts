import { describe, expect, it } from 'vitest'
import { nextFailureIndex } from './next-failure'

describe('nextFailureIndex', () => {
  it('answers -1 when nothing failed', () => {
    expect(nextFailureIndex(['completed', 'queued', 'claimed'], -1)).toBe(-1)
    expect(nextFailureIndex([], -1)).toBe(-1)
  })

  it('finds the first failure when there is no cursor yet', () => {
    expect(nextFailureIndex(['completed', 'failed', 'completed', 'failed'], -1)).toBe(1)
  })

  it('steps forward from the cursor', () => {
    expect(nextFailureIndex(['completed', 'failed', 'completed', 'failed'], 1)).toBe(3)
    expect(nextFailureIndex(['completed', 'failed', 'completed', 'failed'], 2)).toBe(3)
  })

  it('wraps past the end back to the top', () => {
    expect(nextFailureIndex(['completed', 'failed', 'completed', 'failed'], 3)).toBe(1)
  })

  it('returns the only failure even when the cursor is already on it', () => {
    expect(nextFailureIndex(['completed', 'failed', 'completed'], 1)).toBe(1)
  })

  it('counts a dead run as a failure', () => {
    expect(nextFailureIndex(['completed', 'dead', 'cancelled'], -1)).toBe(1)
  })

  it('does not count a cancelled run', () => {
    expect(nextFailureIndex(['cancelled', 'completed'], -1)).toBe(-1)
  })
})
