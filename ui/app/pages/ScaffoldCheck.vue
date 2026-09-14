<script setup lang="ts">
import { useQuery } from '@tanstack/vue-query'
import { apiGet } from '~/api/client'
import type { HealthResponse, VersionResponse } from '~/api/types'
import { useAuthStore } from '~/stores/auth'

/**
 * Scaffold verification, not a screen.
 *
 * It exists so step 1 of the rebuild is provable rather than asserted: if this
 * page renders numbers, then Vue, Nuxt UI, Pinia, vue-query, the ofetch client
 * and the dev proxy are all wired correctly against a real server. It is
 * deleted when the shell lands in step 2.
 *
 * Both endpoints are public, so this works signed out — which is also how it
 * shows the session state without needing a login form yet.
 */
const auth = useAuthStore()

const health = useQuery({
  queryKey: ['health'],
  queryFn: () => apiGet<HealthResponse>('/health'),
  refetchInterval: 5_000,
})

const version = useQuery({
  queryKey: ['version'],
  queryFn: () => apiGet<VersionResponse>('/version'),
  staleTime: Infinity,
})
</script>

<template>
  <UContainer class="py-10">
    <div class="mb-8">
      <h1 class="text-xl font-semibold">
        Croniq — Vue scaffold
      </h1>
      <p class="text-sm text-muted">
        Step 1 of the rebuild (ADR-0004): scaffold and data layer. Numbers below
        come from the live API through the same client the real screens will
        use.
      </p>
    </div>

    <div class="grid gap-4 sm:grid-cols-2">
      <UCard>
        <template #header>
          <div class="flex items-center justify-between">
            <span class="font-medium">Health</span>
            <UBadge
              :color="health.isError.value ? 'error' : health.data.value ? 'success' : 'neutral'"
              variant="subtle"
            >
              {{ health.isError.value ? 'unreachable' : health.data.value?.status ?? 'loading' }}
            </UBadge>
          </div>
        </template>

        <dl
          v-if="health.data.value"
          class="grid grid-cols-2 gap-y-2 text-sm"
        >
          <dt class="text-muted">
            Runners online
          </dt>
          <dd class="text-right tabular-nums">
            {{ health.data.value.runners_online }}
          </dd>
          <dt class="text-muted">
            Runners stale
          </dt>
          <dd class="text-right tabular-nums">
            {{ health.data.value.runners_stale }}
          </dd>
          <dt class="text-muted">
            Queued
          </dt>
          <dd class="text-right tabular-nums">
            {{ health.data.value.queued }}
          </dd>
        </dl>
        <p
          v-else-if="health.isError.value"
          class="text-sm text-muted"
        >
          No server on the proxy target. Start one — see scripts/dev-stack.mjs.
        </p>
      </UCard>

      <UCard>
        <template #header>
          <span class="font-medium">Session</span>
        </template>
        <dl class="grid grid-cols-2 gap-y-2 text-sm">
          <dt class="text-muted">
            Status
          </dt>
          <dd class="text-right font-mono">
            {{ auth.status }}
          </dd>
          <dt class="text-muted">
            Access token
          </dt>
          <dd class="text-right font-mono">
            {{ auth.token ? 'in memory' : 'none' }}
          </dd>
          <dt class="text-muted">
            Server version
          </dt>
          <dd class="text-right font-mono">
            {{ version.data.value?.version ?? '—' }}
          </dd>
        </dl>
        <p class="mt-3 text-xs text-muted">
          `unknown` before the bootstrap refresh answers, then `anonymous` or
          `authenticated`. The access token is never persisted (ADR-0001).
        </p>
      </UCard>
    </div>
  </UContainer>
</template>
