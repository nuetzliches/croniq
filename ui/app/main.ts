import './assets/css/main.css'

import { createApp } from 'vue'
import { createPinia } from 'pinia'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import ui from '@nuxt/ui/vue-plugin'

import App from './App.vue'
import { installAuthWatch, router } from './router'

const app = createApp(App)

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // The client refreshes and retries a 401 itself (api/client.ts), so a
      // query-level retry would multiply refreshes rather than help. Anything
      // else that fails is either a real server error or a bug; retrying hides
      // both.
      retry: false,
      staleTime: 5_000,
    },
  },
})

app.use(createPinia())
app.use(router)
app.use(ui)
app.use(VueQueryPlugin, { queryClient })

app.mount('#app')

// After the first navigation resolves, and after pinia so the store exists.
//
// The watch reads `router.currentRoute` to decide whether the page it would
// redirect away from is public. Before the initial navigation finalises that
// is `START_LOCATION`, whose `meta` is empty — so a bootstrap 401 arriving
// during a cold load of `/invitations/accept?token=…` would redirect to
// `/login` and lose the token (issue #665). `beforeEach` covers that window
// properly, because it is handed the target route rather than having to ask
// for it.
//
// `catch` rather than nothing: a failed initial navigation (a chunk that would
// not load) still leaves a running app, and a session dying in it should still
// be noticed.
void router.isReady().catch(() => undefined).finally(installAuthWatch)
