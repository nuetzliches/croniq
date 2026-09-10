import { createRouter, createWebHistory } from 'vue-router'

/**
 * Routes are added as screens land. The target shape is in
 * `docs/ui-screen-inventory.md`; this file grows toward it rather than being
 * stubbed out in advance, so an unimplemented route 404s honestly instead of
 * rendering an empty page that looks finished.
 */
export const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/',
      name: 'scaffold',
      component: () => import('~/pages/ScaffoldCheck.vue'),
    },
  ],
})
