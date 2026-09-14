import js from '@eslint/js'
import globals from 'globals'
import pluginVue from 'eslint-plugin-vue'
import { defineConfigWithVueTs, vueTsConfigs } from '@vue/eslint-config-typescript'

export default defineConfigWithVueTs(
  // `app/lib/wasm/` is wasm-bindgen output from `scripts/build-wasm.mjs`, not
  // source. Its own `eslint-disable` directives are tuned for a stricter
  // config than this one, so linting it produces unused-directive noise.
  { ignores: ['dist', 'node_modules', 'app/lib/wasm', 'playwright-report', 'test-results'] },
  js.configs.recommended,
  pluginVue.configs['flat/recommended'],
  vueTsConfigs.recommended,
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
