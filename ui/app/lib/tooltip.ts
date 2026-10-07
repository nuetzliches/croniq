import type { ObjectDirective } from 'vue'

/**
 * `v-tooltip="text"`: the dashboard's replacement for the `title` attribute.
 *
 * A native title shows after a long, browser-chosen delay, in the operating
 * system's style, cut off at whatever width the browser likes — and on a dense
 * screen it is the only way to read a truncated error. One element serves
 * every tooltip in the app, and the listeners sit on the document rather than
 * on each host: the live timeline alone has hundreds of hosts, and their text
 * changes every second.
 *
 * A host carries its text in `data-cq-tip`, so the delegated listeners find it
 * with `closest()`. A falsy value means no tooltip, as `:title="undefined"`
 * did.
 *
 * A host with no accessible name of its own (an icon-only button) gets the
 * text as its `aria-label`, because that is what the title it replaced gave
 * screen readers.
 */

export type TooltipValue = string | null | undefined | false

const ATTR = 'data-cq-tip'
/** Marks an `aria-label` this module set, so it can update and remove it. */
const OWNED_LABEL = 'data-cq-tip-label'
/** Before the first tooltip shows. Moving between hosts after that is instant. */
const SHOW_DELAY_MS = 350
/** How long a tooltip stays "warm" after hiding, so the next one skips the delay. */
const WARM_MS = 300
const GAP_PX = 6
const EDGE_PX = 4

let tip: HTMLDivElement | null = null
let host: HTMLElement | null = null
let showTimer: ReturnType<typeof setTimeout> | undefined
let warmUntil = 0
let installed = false

function element(): HTMLDivElement {
  if (!tip) {
    tip = document.createElement('div')
    tip.id = 'cq-tooltip'
    tip.className = 'cq-tooltip'
    tip.setAttribute('role', 'tooltip')
    tip.hidden = true
    document.body.appendChild(tip)
  }
  return tip
}

function textOf(el: Element): string | null {
  return el.getAttribute(ATTR) || null
}

/** Above the host, centred, kept inside the viewport; below if there is no room above. */
function place(target: HTMLElement) {
  const box = element()
  const anchor = target.getBoundingClientRect()
  const { width, height } = box.getBoundingClientRect()
  const viewportWidth = document.documentElement.clientWidth
  let top = anchor.top - height - GAP_PX
  if (top < EDGE_PX) top = anchor.bottom + GAP_PX
  let left = anchor.left + anchor.width / 2 - width / 2
  left = Math.min(Math.max(EDGE_PX, left), viewportWidth - width - EDGE_PX)
  box.style.top = `${Math.round(top)}px`
  box.style.left = `${Math.round(Math.max(EDGE_PX, left))}px`
}

function show(target: HTMLElement) {
  const text = textOf(target)
  if (!text) return
  const box = element()
  host = target
  box.textContent = text
  box.hidden = false
  place(target)
  target.setAttribute('aria-describedby', box.id)
}

function hide() {
  clearTimeout(showTimer)
  showTimer = undefined
  if (!host) return
  host.removeAttribute('aria-describedby')
  host = null
  element().hidden = true
  warmUntil = Date.now() + WARM_MS
}

function schedule(target: HTMLElement) {
  if (target === host) return
  const warm = host !== null || Date.now() < warmUntil
  hide()
  if (warm) {
    show(target)
  } else {
    showTimer = setTimeout(() => show(target), SHOW_DELAY_MS)
  }
}

function hostOf(node: EventTarget | null): HTMLElement | null {
  return node instanceof Element ? node.closest<HTMLElement>(`[${ATTR}]`) : null
}

function install() {
  if (installed || typeof document === 'undefined') return
  installed = true
  document.addEventListener('pointerover', (event) => {
    if (event.pointerType === 'touch') return
    const target = hostOf(event.target)
    if (target) schedule(target)
  })
  document.addEventListener('pointerout', (event) => {
    const from = hostOf(event.target)
    if (!from) return
    // Still inside the same host (moving between its children), or into a
    // nested host, which `pointerover` takes care of.
    if (event.relatedTarget instanceof Node && from.contains(event.relatedTarget)) return
    if (from === host || showTimer !== undefined) hide()
  })
  document.addEventListener('focusin', (event) => {
    const target = hostOf(event.target)
    if (target && target.matches(':focus-visible')) schedule(target)
  })
  document.addEventListener('focusout', hide)
  document.addEventListener('pointerdown', hide, true)
  document.addEventListener('scroll', hide, true)
  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') hide()
  })
}

function hasOwnName(el: HTMLElement): boolean {
  if (el.hasAttribute('aria-labelledby')) return true
  if (el.hasAttribute('aria-label') && !el.hasAttribute(OWNED_LABEL)) return true
  return (el.textContent ?? '').trim() !== ''
}

function apply(el: HTMLElement, value: TooltipValue) {
  install()
  const text = value ? String(value) : ''
  if (text) el.setAttribute(ATTR, text)
  else el.removeAttribute(ATTR)

  if (text && !hasOwnName(el)) {
    el.setAttribute('aria-label', text)
    el.setAttribute(OWNED_LABEL, '')
  } else if (el.hasAttribute(OWNED_LABEL)) {
    el.removeAttribute('aria-label')
    el.removeAttribute(OWNED_LABEL)
  }

  if (el === host) {
    if (text) {
      element().textContent = text
      place(el)
    } else {
      hide()
    }
  }
}

export const vTooltip: ObjectDirective<HTMLElement, TooltipValue> = {
  mounted: (el, binding) => apply(el, binding.value),
  updated: (el, binding) => {
    if (binding.value !== binding.oldValue) apply(el, binding.value)
  },
  beforeUnmount: (el) => {
    if (el === host) hide()
  },
}

declare module 'vue' {
  interface GlobalDirectives {
    vTooltip: typeof vTooltip
  }
}
