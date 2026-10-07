import { describe, expect, it } from 'vitest'
import { clampPaneWidth } from './pane-width'

describe('clampPaneWidth', () => {
  it('keeps a width inside the bounds', () => {
    expect(clampPaneWidth(500, 320, 800)).toBe(500)
  })

  it('raises a width below the minimum', () => {
    expect(clampPaneWidth(100, 320, 800)).toBe(320)
  })

  it('lowers a width above the maximum', () => {
    expect(clampPaneWidth(1200, 320, 800)).toBe(800)
  })

  it('gives the minimum when the window leaves less room than that', () => {
    expect(clampPaneWidth(500, 320, 200)).toBe(320)
  })
})
