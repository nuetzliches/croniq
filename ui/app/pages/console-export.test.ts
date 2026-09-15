import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * What the console's two export paths actually serialise.
 *
 * #702 changed `filtered` from a list of `LogEvent` to a list of row
 * objects — `{ event, time, fields, gutter }` — so the template could stop
 * recomputing per patch. The copy path was updated to unwrap the row; the
 * NDJSON export was not, so it wrote a CSS class name and a pre-sliced
 * timestamp into records meant for a program to read (issue #723).
 *
 * That slipped through because the change was mechanical and the export has no
 * test. It has one now, and it reads the source rather than running the
 * download — `URL.createObjectURL` and an anchor click are not worth a DOM
 * harness to assert one `.map`.
 *
 * What it pins is the invariant: neither export may serialise a row.
 */

const SOURCE = readFileSync(join(import.meta.dirname, 'ConsoleView.vue'), 'utf8')

/** The body of a named function in the script block. */
function functionBody(name: string): string {
  const start = SOURCE.indexOf(`function ${name}(`)
  expect(start, `${name} not found — was it renamed?`).toBeGreaterThan(-1)
  const next = SOURCE.indexOf('\nfunction ', start + 1)
  return SOURCE.slice(start, next === -1 ? undefined : next)
}

describe('the console export paths', () => {
  it('writes NDJSON from the log events, not the row wrappers', () => {
    const body = functionBody('downloadNdjson')

    // A row carries presentation — `gutter` is a CSS class. Serialising it
    // puts styling into a machine-readable record.
    expect(body).toContain('row.event')
    expect(body, 'a bare `filtered.value.map((event) => …)` serialises rows').not.toMatch(
      /filtered\.value\s*\.?\s*map\(\(event\)/,
    )
  })

  it('strips the sequence number, which the server never sent', () => {
    // `seq` is assigned on arrival to key the rows (#671). It is ours, so it
    // does not belong in an export that claims to carry what the server said.
    expect(functionBody('downloadNdjson')).toContain('seq')
  })

  it('copies the rendered text from the events too', () => {
    expect(functionBody('copyAll')).toContain('row.event')
  })
})
