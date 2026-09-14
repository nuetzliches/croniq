/**
 * Types for `stack.mjs`, which the dashboard's TypeScript configs import.
 *
 * `vite.config.ts` and `playwright.config.ts` read `PORTS` from it, and
 * `tsc` will not take an untyped `.mjs` — so the choice is this file or
 * `allowJs` across the whole tree. This is the smaller of the two, and it
 * documents the surface the TypeScript side is allowed to depend on: the rest
 * of the module is for the Node scripts, which do not type-check.
 */

/** Croniq's development port block, with the environment overrides applied. */
export declare const PORTS: {
  /** Dev API. `CRONIQ_DEV_PORT`. */
  readonly api: number
  /** Dev dashboard, served by vite. `CRONIQ_DEV_UI_PORT`. */
  readonly ui: number
  /** The e2e stack's server. `CRONIQ_E2E_PORT`. */
  readonly e2e: number
}

/** The repository root, resolved from this module's own location. */
export declare const ROOT: string
