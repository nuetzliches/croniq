<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { apiPost } from '~/api/client'
import { useAuthConfig } from '~/api/queries'
import {
  isEnrollmentRequired,
  isMfaRequired,
  type LoginResponse,
  type TokenResponse,
  type TotpSetupResponse,
} from '~/api/types'
import { useAuthStore } from '~/stores/auth'

/**
 * The login form.
 *
 * Deliberately *only* the flow. The React page is 1,096 lines, most of it a
 * simulated console animation and a rotating tagline; none of that is
 * behaviour and none of it is carried over.
 *
 * The flow itself is carried over completely, because every branch of it
 * exists for a reason the server enforces:
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

/**
 * Shown from the start when the server enforces 2FA, so an enforced login is
 * one round trip rather than two.
 */
const totpUpFront = computed(() => config.value?.totp.required === true)

const codeLabel = computed(() => (useRecovery.value ? 'Recovery code' : 'Two-factor code'))

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
      // Ask for the refresh token as an HttpOnly cookie (ADR-0001). Always
      // true here: this build is same-origin only, so there is no case where
      // the cookie cannot be delivered.
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
  // Deliberately the same message for both: telling an unauthenticated caller
  // whether the username exists is a user-enumeration oracle.
  if (status === 401) return 'Wrong username, password, or code.'
  if (status === 429) return 'Too many attempts from this address. Try again shortly.'
  return (caught as Error).message || 'Sign-in failed.'
}
</script>

<template>
  <div class="flex min-h-screen items-center justify-center bg-default p-6">
    <UCard class="w-full max-w-sm">
      <template #header>
        <div class="flex items-center gap-2">
          <BrandMark
            :size="20"
            chip
          />
          <span class="font-semibold">Sign in to Croniq</span>
        </div>
      </template>

      <!--
        role="alert" so the message is announced, not merely rendered. The
        errors here are the ones a user has to react to.
      -->
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
        <!--
          Every control gets an accessible name (#595). UFormField's `label`
          wires `for`/`id` for us, which is exactly what the React tree's
          hand-rolled fields do not do.
        -->
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

      <!--
        Enforced 2FA with no confirmed secret. Enrolling inline is the
        difference between a first login and a locked-out account.
      -->
      <div
        v-else
        class="flex flex-col gap-4"
      >
        <p class="text-sm text-muted">
          This server requires two-factor authentication. Scan the secret with an
          authenticator app, then enter the code it shows.
        </p>
        <div class="rounded-md border border-default p-3">
          <p class="mb-1 text-xs text-muted">
            Secret
          </p>
          <code class="font-mono text-sm break-all">{{ enrolment?.secret }}</code>
        </div>

        <!--
          The recovery codes are shown here and never again — the server keeps
          only hashes. Asking someone to confirm they have saved codes they
          were never shown is worse than not asking, so this block and the
          checkbox below belong together.
        -->
        <div class="rounded-md border border-default p-3">
          <p class="mb-2 text-xs text-muted">
            Recovery codes — each works once, and this is the only time they are shown.
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
        <!--
          `login_url` comes from the server rather than being assembled here:
          it carries the provider's authorize endpoint and the state parameter,
          neither of which the client may invent.
        -->
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
  </div>
</template>
