<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { apiPost } from '~/api/client'
import { useAuthConfig, useHealth, useVersion } from '~/api/queries'
import {
  isEnrollmentRequired,
  isMfaRequired,
  type LoginResponse,
  type TokenResponse,
  type TotpSetupResponse,
} from '~/api/types'
import { useAuthStore } from '~/stores/auth'

/**
 * Sign in.
 *
 * The shipping dashboard makes this a product page rather than a form —
 * headline, positioning, and live figures pulled from the public `/health`
 * before anyone has credentials — and the audit
 * (docs/ui-visual-design.md) found that to be one of its better ideas: on a
 * self-hosted tool, the first question is "is this thing even up", and the
 * login page can answer it. Carried over.
 *
 * Not carried over: the simulated terminal that types a command nobody runs.
 * It is charming and it is fake, and on an operations tool the honest version
 * of the same idea — the server's actual state — is already on the page.
 *
 * The flow itself is complete, because every branch of it is something the
 * server enforces:
 *
 *   password            → tokens
 *   password + 2FA on   → `requires_totp`, resubmit with a code
 *   password + enforced → `enrollment_required`, enrol inline rather than
 *                         locking the account out
 *   OIDC                → redirect
 */
const auth = useAuthStore()
const router = useRouter()
const route = useRoute()
const { data: config } = useAuthConfig()
const { data: health, isError: healthFailed } = useHealth()
const { data: version } = useVersion()

type Step = 'credentials' | 'totp' | 'enrol'

const step = ref<Step>('credentials')
const username = ref('')
const password = ref('')
const code = ref('')
const useRecovery = ref(false)
const acknowledged = ref(false)
const enrolment = ref<TotpSetupResponse | null>(null)
const error = ref('')
const busy = ref(false)

const passwordEnabled = computed(() => config.value?.password.enabled !== false)
const oidcEnabled = computed(() => config.value?.oidc.enabled === true)

/** Shown from the start when the server enforces 2FA, so an enforced login is
 *  one round trip rather than two. */
const totpUpFront = computed(() => config.value?.totp.required === true)
const codeLabel = computed(() => (useRecovery.value ? 'Recovery code' : 'Two-factor code'))

/** Real numbers from the public endpoint. Nothing here is illustrative. */
const stats = computed(() => [
  {
    label: 'Status',
    value: healthFailed.value ? 'unreachable' : (health.value?.status ?? '…'),
    tone: healthFailed.value ? 'error' : 'success',
  },
  {
    label: 'Runners',
    value: health.value ? String(health.value.runners_online) : '…',
    sub: health.value?.runners_stale ? `${health.value.runners_stale} stale` : 'online',
  },
  {
    label: 'Queued',
    value: health.value ? String(health.value.queued) : '…',
    sub: health.value?.queued ? 'awaiting a runner' : 'nothing waiting',
  },
])

function finish(tokens: TokenResponse) {
  auth.setToken(tokens.access_token)
  const next = typeof route.query.next === 'string' ? route.query.next : '/'
  void router.replace(next)
}

async function submitCredentials() {
  error.value = ''
  busy.value = true
  try {
    const body: Record<string, unknown> = {
      username: username.value,
      password: password.value,
      // The refresh token as an HttpOnly cookie (ADR-0001). Always true: this
      // build is same-origin only, so there is no case where it cannot be
      // delivered.
      refresh_cookie: true,
    }
    const trimmed = code.value.trim()
    if (trimmed) {
      if (useRecovery.value) body.recovery_code = trimmed
      else body.code = trimmed
    }

    const response = await apiPost<LoginResponse>('/v1/auth/login', body)

    if (isMfaRequired(response)) {
      step.value = 'totp'
      error.value = useRecovery.value
        ? 'Enter one of your recovery codes.'
        : 'Enter the 6-digit code from your authenticator app.'
      return
    }
    if (isEnrollmentRequired(response)) {
      enrolment.value = await apiPost<TotpSetupResponse>('/v1/auth/login/enroll/totp/begin', {
        enroll_token: response.enroll_token,
      })
      step.value = 'enrol'
      return
    }
    finish(response)
  } catch (caught) {
    error.value = messageFor(caught)
    code.value = ''
  } finally {
    busy.value = false
  }
}

async function confirmEnrolment() {
  error.value = ''
  busy.value = true
  try {
    const tokens = await apiPost<TokenResponse>('/v1/auth/login/enroll/totp/confirm', {
      code: code.value.trim(),
    })
    finish(tokens)
  } catch (caught) {
    error.value = messageFor(caught)
    code.value = ''
  } finally {
    busy.value = false
  }
}

function messageFor(caught: unknown): string {
  const status = (caught as { status?: number }).status
  // The same message for a wrong username and a wrong password on purpose:
  // telling an unauthenticated caller which one was wrong is a user-
  // enumeration oracle.
  if (status === 401) return 'Wrong username, password, or code.'
  if (status === 429) return 'Too many attempts from this address. Try again shortly.'
  return (caught as Error).message || 'Sign-in failed.'
}
</script>

<template>
  <div class="min-h-screen">
    <div class="mx-auto grid min-h-screen max-w-6xl items-center gap-10 px-6 py-10 lg:grid-cols-[1.1fr_minmax(22rem,26rem)]">
      <!-- The product half. Hidden on narrow screens: on a phone the only
           thing anyone came here to do is sign in. -->
      <section class="hidden lg:block">
        <div class="mb-8 flex items-center gap-2.5">
          <BrandMark
            :size="26"
            chip
          />
          <span class="text-lg font-semibold tracking-tight">Croniq</span>
          <UBadge
            v-if="version?.version"
            color="neutral"
            variant="subtle"
            size="sm"
            class="ml-1 font-mono"
          >
            v{{ version.version }}
          </UBadge>
        </div>

        <h1 class="text-4xl leading-tight font-semibold tracking-tight text-highlighted">
          Schedule. Observe.<br>
          <span class="text-primary">Recover.</span>
        </h1>
        <p class="mt-4 max-w-md text-toned">
          Self-hosted cron for fleets that outgrew the crontab. A typed DSL,
          capability-routed runners, calendars, dead-letter triage and an audit
          log on every mutation.
        </p>

        <!-- Live, from /health. The first question about a self-hosted tool is
             whether it is up, and this page can answer it before you have
             credentials to ask with. -->
        <dl class="mt-8 grid max-w-lg grid-cols-3 gap-3">
          <div
            v-for="stat in stats"
            :key="stat.label"
            class="rounded-xl border border-default bg-default p-4 shadow-sm"
          >
            <dt class="cq-label">
              {{ stat.label }}
            </dt>
            <dd
              class="mt-1 text-2xl font-semibold cq-num"
              :class="stat.tone === 'error' ? 'text-error' : 'text-highlighted'"
            >
              {{ stat.value }}
            </dd>
            <dd
              v-if="stat.sub"
              class="mt-0.5 text-xs text-muted"
            >
              {{ stat.sub }}
            </dd>
          </div>
        </dl>

        <p
          v-if="version"
          class="mt-6 font-mono text-xs text-dimmed"
        >
          build {{ version.git_sha }} · {{ new Date(version.build_time).toISOString().slice(0, 10) }}
        </p>
      </section>

      <!-- The form half. -->
      <section>
        <UCard :ui="{ body: 'p-6 sm:p-6' }">
          <div class="mb-5 lg:hidden">
            <BrandMark
              :size="26"
              chip
            />
          </div>
          <h2 class="text-xl font-semibold tracking-tight text-highlighted">
            Welcome back
          </h2>
          <p class="mt-1 mb-5 text-sm text-muted">
            Sign in to continue.
          </p>

          <!-- role="alert" so the message is announced, not merely rendered. -->
          <UAlert
            v-if="error"
            :description="error"
            color="error"
            variant="subtle"
            role="alert"
            class="mb-4"
          />

          <form
            v-if="step !== 'enrol'"
            class="flex flex-col gap-4"
            @submit.prevent="submitCredentials"
          >
            <!-- UFormField wires for/id, so every control has an accessible
                 name from the first screen (#595). -->
            <UFormField
              label="Username"
              name="username"
            >
              <UInput
                v-model="username"
                autocomplete="username"
                autofocus
                required
                class="w-full"
              />
            </UFormField>

            <UFormField
              label="Password"
              name="password"
            >
              <UInput
                v-model="password"
                type="password"
                autocomplete="current-password"
                required
                class="w-full"
              />
            </UFormField>

            <UFormField
              v-if="step === 'totp' || totpUpFront"
              :label="codeLabel"
              name="code"
              :description="
                totpUpFront
                  ? 'Two-factor is required to sign in.'
                  : 'Required because two-factor is enabled on this account.'
              "
            >
              <UInput
                v-model="code"
                autocomplete="one-time-code"
                inputmode="text"
                class="w-full font-mono"
              />
              <UButton
                variant="link"
                size="xs"
                class="mt-1 px-0"
                @click="
                  () => {
                    useRecovery = !useRecovery
                    code = ''
                    error = ''
                  }
                "
              >
                {{ useRecovery ? 'Use an authenticator code instead' : 'Use a recovery code instead' }}
              </UButton>
            </UFormField>

            <UButton
              type="submit"
              block
              size="lg"
              trailing-icon="i-lucide-arrow-right"
              :loading="busy"
              :disabled="!passwordEnabled"
            >
              Sign in
            </UButton>

            <p
              v-if="!passwordEnabled"
              class="text-sm text-muted"
            >
              Password sign-in is disabled on this server.
            </p>
          </form>

          <!-- Enforced 2FA with no confirmed secret. Enrolling inline is the
               difference between a first login and a locked-out account. -->
          <div
            v-else
            class="flex flex-col gap-4"
          >
            <p class="text-sm text-muted">
              This server requires two-factor authentication. Add the secret to an
              authenticator app, then enter the code it shows.
            </p>
            <div class="rounded-lg border border-default p-3">
              <p class="cq-label mb-1">
                Secret
              </p>
              <code class="font-mono text-sm break-all">{{ enrolment?.secret }}</code>
            </div>

            <!-- Shown here and never again: the server keeps only hashes.
                 Asking someone to confirm they saved codes they were never
                 shown is worse than not asking. -->
            <div class="rounded-lg border border-default p-3">
              <p class="cq-label mb-2">
                Recovery codes — each works once
              </p>
              <ul class="grid grid-cols-2 gap-1 font-mono text-sm">
                <li
                  v-for="recoveryCode in enrolment?.recovery_codes ?? []"
                  :key="recoveryCode"
                >
                  {{ recoveryCode }}
                </li>
              </ul>
            </div>

            <UFormField
              label="Verification code"
              name="enrol-code"
            >
              <UInput
                v-model="code"
                inputmode="numeric"
                autocomplete="one-time-code"
                class="w-full font-mono"
              />
            </UFormField>
            <UCheckbox
              v-model="acknowledged"
              label="I have saved my recovery codes"
            />
            <UButton
              block
              size="lg"
              :loading="busy"
              :disabled="!acknowledged"
              @click="confirmEnrolment"
            >
              Confirm and sign in
            </UButton>
          </div>

          <template
            v-if="oidcEnabled"
            #footer
          >
            <!-- `login_url` comes from the server: it carries the provider's
                 authorize endpoint and the state parameter, neither of which
                 the client may invent. -->
            <UButton
              block
              color="neutral"
              variant="subtle"
              icon="i-lucide-key-round"
              :to="config?.oidc.login_url ?? undefined"
              :disabled="!config?.oidc.login_url"
              external
            >
              {{
                config?.oidc.provider_name
                  ? `Sign in with ${config.oidc.provider_name}`
                  : 'Sign in with SSO'
              }}
            </UButton>
          </template>
        </UCard>
      </section>
    </div>
  </div>
</template>
