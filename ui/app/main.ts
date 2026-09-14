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

// After the pinia plugin is installed, so the store exists. See the comment on
// installAuthWatch: a guard alone does not notice a session dying in place.
installAuthWatch()

app.mount('#app')
