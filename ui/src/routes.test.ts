/**
 * @vitest-environment jsdom
 */
import { isValidElement } from 'react'
import { describe, expect, it, vi } from 'vitest'
import { createRoutesFromElements, matchRoutes } from 'react-router'
import type { RouteObject } from 'react-router'
// Importing the route table pulls in Layout, which reaches `lib/theme` — that
// reads localStorage and paints the theme onto <html> at module scope. Hence
// the jsdom environment above (the rest of the suite stays on `node`).
// matchMedia is the one browser API jsdom does not implement, so stub it
// before the imports run.
vi.hoisted(() => {
  vi.stubGlobal('matchMedia', () => ({
    matches: false,
    addEventListener: () => {},
    removeEventListener: () => {},
  }))
})

import { appRoutes } from './routes'
import { LoginPage } from '@/auth/LoginPage'
import { ProtectedRoute } from '@/auth/ProtectedRoute'
import { Layout } from '@/layout/Layout'
import { NotFoundPage } from '@/pages/NotFoundPage'

// `matchRoutes` resolves a path against the real route table without rendering
// anything, so no testing-library is needed. It answers exactly the question
// the blank-page bug got wrong: which components does a given URL resolve to?
const routes = createRoutesFromElements(appRoutes)

/** The component chain a path resolves to, outermost first. */
function chainFor(pathname: string): unknown[] {
  const matches = matchRoutes(routes, pathname)
  expect(matches, `no route matched ${pathname}`).not.toBeNull()
  return matches!.map((m) => elementTypeOf(m.route))
}

function elementTypeOf(route: RouteObject): unknown {
  return isValidElement(route.element) ? route.element.type : undefined
}

describe('route table', () => {
  it.each([
    '/does-not-exist',
    '/jobs/foo/bar/baz', // deeper than any real route
    '/settings/tokens', // a sub-path of a real page that was never a route
  ])('renders the 404 page inside the layout for %s', (pathname) => {
    // The regression: without a catch-all these matched nothing at all and
    // <Routes> rendered null — a white page with no navigation and no way back.
    expect(chainFor(pathname)).toEqual([ProtectedRoute, Layout, NotFoundPage])
  })

  it('keeps the unknown path behind the auth gate', () => {
    // ProtectedRoute is the outermost match, so a logged-out visitor hitting a
    // bad URL lands on /login rather than on a 404 page they cannot use.
    expect(chainFor('/nope')[0]).toBe(ProtectedRoute)
  })

  it('does not swallow /login', () => {
    expect(chainFor('/login')).toEqual([LoginPage])
  })

  it.each([
    ['/', '/'],
    ['/jobs', '/jobs'],
    ['/jobs/runner:cleanup', '/jobs/:jobKey'],
    ['/executions', '/executions'],
    ['/executions/0198f0b2', '/executions/:id'],
    ['/dead-letters/42', '/dead-letters/:id'],
    ['/settings', '/settings'],
  ])('still routes %s to %s', (pathname, expected) => {
    const matches = matchRoutes(routes, pathname)
    expect(matches?.at(-1)?.route.path).toBe(expected)
  })
})
