import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * No destructive mutation fires straight from a click.
 *
 * Four of them did: removing a user, deleting an API client, removing a runner
 * and deleting a schedule (issue #667). Deleting an API client revokes every
 * token minted under it; removing a runner orphans its in-flight claims. Both
 * happened on one mis-click, with no undo and nothing asked.
 *
 * A component test would be the direct way to assert this, and there is no
 * harness for one here yet (#677). This reads the templates instead, which is
 * cruder but catches the thing that actually went wrong — a delete wired
 * straight to a handler — and keeps catching it for components nobody has
 * written yet. The nginx CSP drift test in `api::hardening` is the same idea.
 */

const APP = join(import.meta.dirname, '..')

/** Every `.vue` file under `app/`. */
function vueFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry)
    if (statSync(path).isDirectory()) return vueFiles(path)
    return path.endsWith('.vue') ? [path] : []
  })
}

/**
 * A click handler that calls a destructive mutation directly.
 *
 * Deliberately narrow: it looks for `delete`/`remove`/`discard` and
 * `mutateAsync`/`mutate` inside one `@click`, which is the shape all four
 * defects had. A handler that opens a dialog does not match, and neither does
 * a named function — those are read by a human, and the dialog is visible in
 * the same file.
 */
const DESTRUCTIVE_CLICK = /@click="[^"]*\b(delete|remove|discard)\w*\.mutate(Async)?\(/i

describe('destructive controls', () => {
  it('never call a delete mutation straight from a click handler', () => {
    const offenders = vueFiles(APP)
      .map((path) => ({ path, source: readFileSync(path, 'utf8') }))
      .filter(({ source }) => DESTRUCTIVE_CLICK.test(source))
      .map(({ path }) => path.slice(APP.length + 1).replace(/\\/g, '/'))

    expect(
      offenders,
      'a destructive mutation belongs behind ConfirmModal, not on a click',
    ).toEqual([])
  })

  it('every screen that deletes something has a confirmation in it', () => {
    // The mutations that destroy something an operator cannot recreate from
    // the screen they are on. `useDeleteDeadLetter` is absent deliberately:
    // one dead letter is a queue entry, and the view discards them in bulk
    // behind a dialog already.
    const destructive = [
      'useDeleteUser',
      'useDeleteApiClient',
      'useDeleteRunner',
      'useDeleteSchedule',
      'useDeleteJob',
      'useDeleteCalendar',
    ]

    const missing = vueFiles(APP)
      .map((path) => ({ path, source: readFileSync(path, 'utf8') }))
      .filter(({ source }) => destructive.some((hook) => source.includes(hook)))
      .filter(({ source }) => !source.includes('ConfirmModal'))
      .map(({ path }) => path.slice(APP.length + 1).replace(/\\/g, '/'))

    expect(missing, 'these delete something and ask nothing first').toEqual([])
  })
})
