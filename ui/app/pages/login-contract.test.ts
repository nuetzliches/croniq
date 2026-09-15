import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * The sign-in screen sends every field its endpoints require.
 *
 * This is the third defect of the same shape in one release: the client posts a
 * body the server's request struct does not match, serde or axum rejects or
 * silently drops it, and nothing on either side notices. #657 dropped a
 * schedule's window, #658 dropped a token's expiry — both were *missing* server
 * fields, so the request succeeded and did nothing. This one is the opposite
 * and worse: `enroll_token` is a required `String`, so axum answers 422 before
 * the handler runs and **nobody can complete a first sign-in** on a server with
 * enforced 2FA (issue #710).
 *
 * The server side of that contract is well covered (`totp_enforcement.rs`). The
 * gap was between the two, so the test belongs between the two.
 *
 * It reads both sources rather than exercising the code, which is crude — but a
 * component harness would not catch this either, because the bug is in what the
 * body contains and not in how the component behaves. What it does catch is the
 * next required field added to one of these structs.
 */

const AUTH_RS = join(
  import.meta.dirname,
  '..',
  '..',
  '..',
  'crates',
  'croniq-server',
  'src',
  'api',
  'auth_endpoints.rs',
)
const LOGIN_VUE = join(import.meta.dirname, 'LoginView.vue')

/**
 * The fields a request struct requires: everything that is not an `Option` and
 * not `#[serde(default)]`.
 */
function requiredFields(rust: string, structName: string): string[] {
  const start = rust.indexOf(`pub struct ${structName} {`)
  if (start === -1) throw new Error(`${structName} not found — was it renamed?`)
  const body = rust.slice(start, rust.indexOf('\n}', start))

  const required: string[] = []
  let defaulted = false
  for (const line of body.split('\n').slice(1)) {
    const trimmed = line.trim()
    if (trimmed.includes('#[serde(default')) {
      defaulted = true
      continue
    }
    const field = /^pub (\w+):\s*(.+?),?$/.exec(trimmed)
    if (!field) continue
    const [, name, type] = field as unknown as [string, string, string]
    if (!defaulted && !type.startsWith('Option<')) required.push(name)
    defaulted = false
  }
  return required
}

/** The object literal passed to `apiPost('<path>', { … })`, as source text. */
function postBody(vue: string, path: string): string {
  const call = vue.indexOf(`'${path}'`)
  expect(call, `LoginView does not post to ${path}`).toBeGreaterThan(-1)
  const open = vue.indexOf('{', call)
  // Far enough to cover the literal and no further; these calls are short.
  return vue.slice(open, vue.indexOf('})', open))
}

describe('the sign-in screen against the server’s request structs', () => {
  const rust = readFileSync(AUTH_RS, 'utf8')
  const vue = readFileSync(LOGIN_VUE, 'utf8')

  it.each([
    ['/v1/auth/login/enroll/totp/begin', 'EnrollTotpBeginRequest'],
    ['/v1/auth/login/enroll/totp/confirm', 'EnrollTotpConfirmRequest'],
  ])('%s carries every field %s requires', (path, structName) => {
    const required = requiredFields(rust, structName)
    expect(required.length, `${structName} has no required fields — check the parser`).toBeGreaterThan(0)

    const body = postBody(vue, path)
    for (const field of required) {
      expect(body, `${path} must send ${field}`).toContain(`${field}:`)
    }
  })

  it.each(['submitCredentials', 'confirmEnrolment'])(
    '%s asks for the refresh cookie',
    (fnName) => {
      // Not required by the struct — it defaults to `false` — but omitting it
      // hands the operator a session that does not survive a reload. They
      // enrol successfully and are signed out on the next page load, which
      // reads as enrolment having failed.
      //
      // Anchored on the function rather than on the request body: `signIn`
      // builds its body as a named variable, so there is no literal beside
      // the path to read.
      const start = vue.indexOf(`async function ${fnName}(`)
      expect(start, `${fnName} not found — was it renamed?`).toBeGreaterThan(-1)
      const next = vue.indexOf(`\nasync function `, start + 1)
      const body = vue.slice(start, next === -1 ? undefined : next)

      expect(body, `${fnName} must ask for the refresh cookie`).toContain(
        'refresh_cookie: true',
      )
    },
  )
})
