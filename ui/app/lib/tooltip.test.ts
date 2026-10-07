// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { DirectiveBinding } from 'vue'
import { vTooltip, type TooltipValue } from './tooltip'

/** The directive's hooks, called the way Vue would, without a component harness (#677). */
function binding(value: TooltipValue, oldValue: TooltipValue = undefined) {
  return { value, oldValue } as DirectiveBinding<TooltipValue>
}

function mount(el: HTMLElement, value: TooltipValue) {
  document.body.appendChild(el)
  ;(vTooltip.mounted as (el: HTMLElement, b: DirectiveBinding<TooltipValue>) => void)(
    el,
    binding(value),
  )
}

function hover(el: Element) {
  el.dispatchEvent(new PointerEvent('pointerover', { bubbles: true, pointerType: 'mouse' }))
}

function tip(): HTMLElement | null {
  return document.getElementById('cq-tooltip')
}

describe('v-tooltip', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => {
    document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    vi.advanceTimersByTime(1_000)
    vi.useRealTimers()
    document.body.querySelectorAll('[data-cq-tip]').forEach((el) => el.remove())
  })

  it('shows the text after a delay and never sets a native title', () => {
    const el = document.createElement('span')
    el.textContent = 'truncated error…'
    mount(el, 'the whole error')
    expect(el.hasAttribute('title')).toBe(false)

    hover(el)
    expect(tip()?.hidden ?? true).toBe(true)
    vi.advanceTimersByTime(400)
    expect(tip()?.hidden).toBe(false)
    expect(tip()?.textContent).toBe('the whole error')
    expect(el.getAttribute('aria-describedby')).toBe('cq-tooltip')
  })

  it('a falsy value means no tooltip', () => {
    const el = document.createElement('span')
    el.textContent = 'x'
    mount(el, undefined)
    expect(el.hasAttribute('data-cq-tip')).toBe(false)
  })

  it('names an icon-only host, and stops when the text goes', () => {
    const el = document.createElement('button')
    mount(el, 'Copy the logs')
    expect(el.getAttribute('aria-label')).toBe('Copy the logs')

    ;(vTooltip.updated as (el: HTMLElement, b: DirectiveBinding<TooltipValue>) => void)(
      el,
      binding(null, 'Copy the logs'),
    )
    expect(el.hasAttribute('aria-label')).toBe(false)
  })

  it('leaves an aria-label the host already has', () => {
    const el = document.createElement('button')
    el.setAttribute('aria-label', 'Mark the mail:send failure as checked')
    mount(el, 'Mark as checked')
    expect(el.getAttribute('aria-label')).toBe('Mark the mail:send failure as checked')
  })

  it('hides on Escape', () => {
    const el = document.createElement('span')
    el.textContent = 'x'
    mount(el, 'text')
    hover(el)
    vi.advanceTimersByTime(400)
    expect(tip()?.hidden).toBe(false)
    document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    expect(tip()?.hidden).toBe(true)
  })
})
