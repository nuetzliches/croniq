import js from '@eslint/js'
import globals from 'globals'
import pluginVue from 'eslint-plugin-vue'
import { defineConfigWithVueTs, vueTsConfigs } from '@vue/eslint-config-typescript'

/** Elements whose `title` ends up as the HTML attribute, i.e. a native tooltip. */
const NATIVE_TITLE_HOSTS =
  '/^(?:[a-z][a-z0-9]*|RouterLink|ULink|UButton|UBadge|UIcon|UKbd|UAvatar)$/'
const NATIVE_TITLE_MESSAGE =
  'Use v-tooltip (app/lib/tooltip.ts) instead of a native title tooltip.'

export default defineConfigWithVueTs(
  // `app/lib/wasm/` is wasm-bindgen output from `scripts/build-wasm.mjs`, not
  // source. Its own `eslint-disable` directives are tuned for a stricter
  // config than this one, so linting it produces unused-directive noise.
  { ignores: ['dist', 'node_modules', 'app/lib/wasm', 'playwright-report', 'test-results'] },
  js.configs.recommended,
  pluginVue.configs['flat/recommended'],
  vueTsConfigs.recommended,
  {
    // No native `title` tooltips: they show late, in the OS style, and cut off
    // wherever the browser likes. `v-tooltip` (app/lib/tooltip.ts) replaces
    // them. Restricted to plain HTML elements (lowercase, no hyphen) and the
    // components that only pass `title` through to their root element, so a
    // component that declares `title` as a prop — AppEmpty, ConfirmModal,
    // UAlert, UModal, SecretOnce, StatusPill — is not flagged.
    files: ['app/**/*.vue'],
    rules: {
      'vue/no-restricted-static-attribute': [
        'error',
        {
          key: 'title',
          element: NATIVE_TITLE_HOSTS,
          message: NATIVE_TITLE_MESSAGE,
        },
      ],
      'vue/no-restricted-v-bind': [
        'error',
        {
          argument: 'title',
          element: NATIVE_TITLE_HOSTS,
          message: NATIVE_TITLE_MESSAGE,
        },
      ],
    },
  },
  {
    // The tooling scripts and the Playwright suite. Node programs that also
    // hand code to a browser through `page.evaluate`, so both global sets
    // apply — a script with only `globals.node` reports every `document` in an
    // evaluated callback as undefined, which is how a linter gets switched off
    // for a directory and then stops catching anything there.
    files: ['scripts/**/*.mjs', 'e2e/**/*.ts', 'playwright.config.ts'],
    languageOptions: {
      globals: { ...globals.node, ...globals.browser },
    },
  },
)
