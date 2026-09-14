<script setup lang="ts">
import { computed, onMounted, onScopeDispose, onUnmounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { apiPost } from '~/api/client'
import { useAuthConfig, useHealth, useVersion } from '~/api/queries'
import { UI_VERSION, versionSkew } from '~/lib/build-version'
import {
  isEnrollmentRequired,
  isMfaRequired,
  type LoginResponse,
  type TokenResponse,
  type TotpSetupResponse,
} from '~/api/types'
import { useAuthStore } from '~/stores/auth'
import { useUiStore } from '~/stores/ui'

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
const ui = useUiStore()
const router = useRouter()
const route = useRoute()
const { data: config } = useAuthConfig()
const { data: health, isError: healthFailed } = useHealth()
const { data: version } = useVersion()

/**
 * A version mismatch is announced in the shell (`VersionSkewBanner`), which is
 * no use at all if the mismatch is what is stopping you signing in. One line
 * here rather than a second banner: this page is deliberately quiet, and an
 * operator who cannot get past it needs the two numbers, not a paragraph.
 */
const skew = computed(() => versionSkew(UI_VERSION, version.value?.version))

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

/** Which server this is. See the card header. */
const host = computed(() => window.location.host)

const resetPending = ref(false)
/** What the reset request said — deliberately the same either way. */
const resetNotice = ref<string | null>(null)

/**
 * Ask for a password-reset link.
 *
 * The server answers 202 whether or not the account exists, and the wording
 * here matches: telling someone "no such user" turns this form into a way to
 * enumerate accounts. The username field is reused rather than asking for an
 * email, because the endpoint takes a username and the address it mails is the
 * one on file.
 *
 * The link lands on `/password-reset/confirm`, which this tree now serves —
 * it did not, in either tree, until the route beside this one was added.
 */
async function requestReset() {
  const name = username.value.trim()
  if (!name) {
    error.value = 'Enter your username first — the link goes to the address on file for it.'
    return
  }
  error.value = ''
  resetPending.value = true
  try {
    await apiPost('/v1/auth/password-reset/request', { username: name })
  } catch {
    // Same notice either way, on purpose: a failure that reads differently
    // from a success is itself an account-enumeration oracle.
  } finally {
    resetPending.value = false
    resetNotice.value =
      'If that account exists, a reset link is on its way. It is good for one hour.'
  }
}
const oidcEnabled = computed(() => config.value?.oidc.enabled === true)

/** Shown from the start when the server enforces 2FA, so an enforced login is
 *  one round trip rather than two. */
const totpUpFront = computed(() => config.value?.totp.required === true)
const codeLabel = computed(() => (useRecovery.value ? 'Recovery code' : 'Two-factor code'))

/** Real numbers from the public endpoint. Nothing here is illustrative. */
/**
 * The third line of the headline, rotating.
 *
 * Croniq is not one verb, and naming five of them in turn says more about the
 * product than a single fixed one — it was in the shipping page and the first
 * Vue login lost it.
 *
 * Stopped entirely under `prefers-reduced-motion`: a word that swaps itself
 * out is exactly the kind of movement that setting exists to refuse, and the
 * page reads perfectly with one.
 */
const VERBS = ['Recover', 'Replay', 'Diagnose', 'Audit', 'Scale']
const VERB_MS = 3500

const verbIndex = ref(0)
const prefersReducedMotion =
  typeof window !== 'undefined' &&
  window.matchMedia('(prefers-reduced-motion: reduce)').matches

if (!prefersReducedMotion) {
  const rotate = setInterval(() => {
    verbIndex.value = (verbIndex.value + 1) % VERBS.length
  }, VERB_MS)
  onScopeDispose(() => clearInterval(rotate))
}

const verb = computed(() => VERBS[verbIndex.value]!)

/**
 * The sign-in page is dark, whatever the reader's theme.
 *
 * It is the one surface in the product that is a front door rather than a
 * tool: a grid on a gradient, a demo console, a headline. All of that is built
 * for a dark ground, and the shipping page has always rendered it that way
 * regardless of the theme setting.
 *
 * Setting only the stage's own background was not enough and is worth
 * recording — every token above it (`text-highlighted`, `bg-default`, the
 * card) resolves from the class on `<html>`, so a dark stage under light
 * tokens produced dark-on-dark text and three white tiles.
 *
 * The reader's choice comes back on the way out via the store, which owns it.
 * Snapshotting the attribute here instead looked equivalent and lost it: on a
 * cold load this `onMounted` runs before the store's theme watcher, so the
 * snapshot was of an attribute nobody had set yet and a reader who had chosen
 * light was handed back nothing.
 */
onMounted(() => {
  const root = document.documentElement
  root.classList.add('dark')
  root.dataset.theme = 'dark'
})

onUnmounted(() => ui.reapplyTheme())

/**
 * The live tiles, each carrying the tone of what it reports.
 *
 * The sub-line is green when the thing is healthy and amber when it is not,
 * which is the difference between a status board and three grey numbers — and
 * is how the shipping page read. `null` before `/health` answers: colouring an
 * unknown state green would be a claim nothing has made yet.
 */
type Tone = 'success' | 'warning' | 'error' | null

const stats = computed<{ label: string; value: string; sub: string; tone: Tone }[]>(() => {
  const live = health.value
  const stale = live?.runners_stale ?? 0
  const dead = live?.runners_dead ?? 0
  return [
    {
      label: 'Queue depth',
      value: live ? String(live.queued) : '…',
      sub: live?.queued ? 'awaiting a runner' : 'awaiting fire',
      tone: live ? 'success' : null,
    },
    {
      label: 'Runners',
      value: live ? `${live.runners_online} / ${live.runners_online + stale + dead}` : '…',
      sub: stale || dead ? `${stale} stale · ${dead} dead` : 'all healthy',
      tone: live ? (stale || dead ? 'warning' : 'success') : null,
    },
    {
      label: 'Status',
      value: healthFailed.value ? 'unreachable' : (live?.status ?? '…'),
      sub: healthFailed.value ? 'check /health' : live ? 'operational' : 'connecting',
      tone: healthFailed.value ? 'error' : live ? 'success' : null,
    },
  ]
})

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
  <div class="cq-stage relative min-h-screen overflow-hidden">
    <!--
      The stage.

      A grid on a gradient, masked to fade at the edges. It is the backdrop the
      shipping sign-in had and the first Vue one did not — and its absence is
      most of why that page read as a form on a flat sheet rather than as the
      front of a product.

      Purely decorative, so `aria-hidden`, and behind everything at z-0.
    -->
    <div
      class="pointer-events-none absolute inset-0 z-0 overflow-hidden"
      aria-hidden="true"
    >
      <!--
        Two drifting spotlights, under the grid.

        Without them the grid is technically painted and practically invisible
        — which is exactly how it was reported. They are what lights it: the
        raster only reads where a spot passes behind it, so the page breathes
        instead of sitting flat.

        Different sizes, colours and periods (90s against 130s) so the two
        never fall into step and the loop never announces itself.
      -->
      <div class="cq-spot cq-spot-a" />
      <div class="cq-spot cq-spot-b" />
      <div class="cq-stage-bg absolute inset-0" />
    </div>

    <div class="relative z-10 mx-auto grid min-h-screen max-w-6xl items-center gap-10 px-6 py-10 lg:grid-cols-[1.1fr_minmax(22rem,26rem)]">
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
          <!--
            `aria-live="off"` and a fixed first word for assistive tech: a
            headline that re-announces itself every 3.5 seconds is worse than
            useless. The rotation is decoration on top of a sentence that
            reads fine without it.
          -->
          <!--
            The two words are stacked in one grid cell so they cross-fade.
            `mode="out-in"` was the obvious way and it is wrong here: the old
            word leaves before the new one arrives, so the line sits visibly
            empty for a beat every few seconds — an aborted render rather than
            a transition.
          -->
          <span class="grid">
            <Transition
              enter-active-class="transition duration-500"
              leave-active-class="transition duration-500"
              enter-from-class="opacity-0 translate-y-2"
              leave-to-class="opacity-0 -translate-y-2"
            >
              <span
                :key="verb"
                class="col-start-1 row-start-1 justify-self-start text-primary"
              >{{ verb }}.</span>
            </Transition>
          </span>
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
              class="mt-0.5 font-mono text-xs"
              :class="{
                'text-success': stat.tone === 'success',
                'text-warning': stat.tone === 'warning',
                'text-error': stat.tone === 'error',
                'text-muted': stat.tone === null,
              }"
            >
              {{ stat.sub }}
            </dd>
          </div>
        </dl>

        <LoginConsole class="mt-8 max-w-lg" />

        <p
          v-if="version"
          class="mt-6 font-mono text-xs text-dimmed"
        >
          build {{ version.git_sha }} · {{ new Date(version.build_time).toISOString().slice(0, 10) }}
        </p>
        <p
          v-if="skew"
          class="mt-1 font-mono text-xs text-warning"
        >
          dashboard v{{ skew.ui }} · server v{{ skew.server }} — versions do not match
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
          <!--
            Which server. An operator with a staging and a production Croniq
            open in two tabs has no other way to tell them apart from inside
            this card, and typing production credentials into staging is a
            mistake the page can simply prevent. Carried over from the
            shipping dashboard, which got this right.
          -->
          <p class="mt-1 mb-5 text-sm text-muted">
            Sign in to
            <span class="font-mono text-default">{{ host }}</span>
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

            <!--
              Lost access.

              This was missing, and it was the one thing on this screen that is
              a capability rather than decoration: without it a person who has
              forgotten their password has no way back in that does not involve
              an administrator and a shell. The server has always had the
              endpoint.

              Deliberately below the sign-in button and quiet: it is the rare
              path, and a recovery link competing with the primary action is
              how people end up resetting a password they still remember.
            -->
            <div
              v-if="passwordEnabled && step === 'credentials'"
              class="flex flex-col items-center gap-2 border-t border-default pt-4"
            >
              <p
                v-if="resetNotice"
                class="text-center text-xs text-muted"
                role="status"
              >
                {{ resetNotice }}
              </p>
              <UButton
                v-else
                variant="link"
                color="neutral"
                size="xs"
                icon="i-lucide-mail"
                :loading="resetPending"
                @click="requestReset"
              >
                Forgot your password?
              </UButton>
            </div>
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

<style scoped>
/*
 * The stage's own ground. The *tokens* above it are switched to dark in the
 * script — see the note there; painting only this left dark text on a dark
 * background and three white tiles.
 */
.cq-stage {
  background:
    radial-gradient(ellipse 80% 60% at 18% 0%, oklch(0.32 0.13 285 / 0.4) 0%, oklch(0.32 0.13 285 / 0) 60%),
    radial-gradient(ellipse 60% 50% at 110% 90%, oklch(0.36 0.18 250 / 0.32) 0%, oklch(0.36 0.18 250 / 0) 55%),
    linear-gradient(180deg, oklch(0.13 0.02 265) 0%, oklch(0.1 0.015 265) 100%);
}

/*
 * The spotlights. `screen` blending so they add light to the ground rather
 * than sitting on it as two visible discs.
 */
.cq-spot {
  position: absolute;
  top: 50%;
  left: 50%;
  border-radius: 50%;
  mix-blend-mode: screen;
  pointer-events: none;
}

.cq-spot-a {
  width: 700px;
  height: 700px;
  margin: -350px 0 0 -350px;
  background: radial-gradient(circle, oklch(0.62 0.18 285 / 0.7) 0%, oklch(0.62 0.18 285 / 0) 100%);
  filter: blur(120px);
  animation: cq-spot-a 90s linear infinite;
}

.cq-spot-b {
  width: 600px;
  height: 600px;
  margin: -300px 0 0 -300px;
  background: radial-gradient(circle, oklch(0.6 0.16 230 / 0.65) 0%, oklch(0.6 0.16 230 / 0) 100%);
  filter: blur(100px);
  animation: cq-spot-b 130s linear infinite;
}

/* Viewport units, so one path suits a wide stage and a narrow one. */
@keyframes cq-spot-a {
  0% { transform: translate(-15vw, -20vh); }
  25% { transform: translate(20vw, -15vh); }
  50% { transform: translate(25vw, 22vh); }
  75% { transform: translate(-10vw, 18vh); }
  100% { transform: translate(-15vw, -20vh); }
}

@keyframes cq-spot-b {
  0% { transform: translate(22vw, 18vh); }
  25% { transform: translate(-18vw, 15vh); }
  50% { transform: translate(-20vw, -18vh); }
  75% { transform: translate(18vw, -20vh); }
  100% { transform: translate(22vw, 18vh); }
}

/*
 * Still, not absent. The lights are what make the grid legible, so removing
 * them under reduced motion would take the background with them — they park
 * at a composed position instead.
 */
@media (prefers-reduced-motion: reduce) {
  .cq-spot-a {
    animation: none;
    transform: translate(-12vw, -16vh);
  }
  .cq-spot-b {
    animation: none;
    transform: translate(18vw, 14vh);
  }
}

/* The grid itself, masked so it fades out rather than ending at an edge. */
.cq-stage-bg {
  background-image:
    linear-gradient(oklch(1 0 0 / 0.04) 1px, transparent 1px),
    linear-gradient(90deg, oklch(1 0 0 / 0.04) 1px, transparent 1px);
  background-size: 36px 36px;
  mask-image: radial-gradient(ellipse 100% 80% at 50% 40%, black 30%, transparent 80%);
}
</style>
