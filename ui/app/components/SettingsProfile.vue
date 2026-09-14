<script setup lang="ts">
import { computed, ref } from 'vue'
import { ApiError } from '~/api/client'
import {
  useCreatePat,
  useCurrentUser,
  usePersonalAccessTokens,
  useRevokePat,
  useTotpConfirm,
  useTotpDisable,
  useTotpSetup,
} from '~/api/queries'
import type { TotpSetupResponse } from '~/api/types'
import { formatAbsolute, formatRelative } from '~/lib/format'

/**
 * You: who you are signed in as, your second factor, and your tokens.
 *
 * Two-factor enrolment is the part that has to be right. `setup` hands back
 * the secret *and* a set of recovery codes, and both are shown exactly once —
 * the React login work found this the hard way, where a checkbox asked the
 * user to confirm they had saved codes that were never rendered. Here both go
 * through `SecretOnce`, which will not let itself be dismissed until they have
 * been copied or explicitly acknowledged.
 */
const { data: me } = useCurrentUser()
const { data: tokens, isPending: tokensPending } = usePersonalAccessTokens()

const setup = useTotpSetup()
const confirm = useTotpConfirm()
const disableTotp = useTotpDisable()
const createPat = useCreatePat()
const revokePat = useRevokePat()

const error = ref<string | null>(null)

/* ─── two-factor ─────────────────────────────────────────────────────────── */

/** The enrolment in progress, or null. Held until `confirm` succeeds. */
const enrolment = ref<TotpSetupResponse | null>(null)
const code = ref('')
const codesAcknowledged = ref(false)
const disablePassword = ref('')
const disabling = ref(false)

async function attempt(fn: () => Promise<unknown>) {
  error.value = null
  try {
    await fn()
    return true
  } catch (caught) {
    const body = caught instanceof ApiError ? (caught.body as { message?: string }) : undefined
    error.value = body?.message ?? (caught as Error).message ?? 'The server refused that.'
    return false
  }
}

async function beginEnrolment() {
  error.value = null
  codesAcknowledged.value = false
  code.value = ''
  try {
    enrolment.value = await setup.mutateAsync()
  } catch (caught) {
    const body = caught instanceof ApiError ? (caught.body as { message?: string }) : undefined
    error.value = body?.message ?? (caught as Error).message ?? 'Could not start enrolment.'
  }
}

async function confirmEnrolment() {
  if (!code.value.trim()) {
    error.value = 'Enter the six-digit code from your authenticator.'
    return
  }
  const ok = await attempt(() => confirm.mutateAsync(code.value.trim()))
  if (ok) {
    enrolment.value = null
    code.value = ''
  }
}

async function doDisable() {
  if (!disablePassword.value) {
    error.value = 'Your password is required to remove the second factor.'
    return
  }
  const ok = await attempt(() => disableTotp.mutateAsync(disablePassword.value))
  if (ok) {
    disablePassword.value = ''
    disabling.value = false
  }
}

/* ─── personal access tokens ─────────────────────────────────────────────── */

const creatingToken = ref(false)
const tokenName = ref('')
const tokenScopes = ref<string[]>([])
const tokenExpiry = ref('')
/** The minted token, shown once. */
const mintedToken = ref<{ name: string; token: string } | null>(null)

async function createToken() {
  if (!tokenName.value.trim()) {
    error.value = 'A token needs a name — it is how you will recognise it in this list.'
    return
  }
  if (tokenScopes.value.length === 0) {
    error.value = 'A token with no scopes can do nothing. Pick at least one.'
    return
  }
  const hours = tokenExpiry.value.trim() ? Number(tokenExpiry.value) : undefined
  if (tokenExpiry.value.trim() && (!Number.isFinite(hours) || hours! <= 0)) {
    error.value = 'Expiry is a number of hours, or empty for no expiry.'
    return
  }
  error.value = null
  try {
    const created = await createPat.mutateAsync({
      name: tokenName.value.trim(),
      scopes: [...tokenScopes.value],
      expires_in_hours: hours,
    })
    mintedToken.value = { name: created.name, token: created.token }
    creatingToken.value = false
    tokenName.value = ''
    tokenScopes.value = []
    tokenExpiry.value = ''
  } catch (caught) {
    const body = caught instanceof ApiError ? (caught.body as { message?: string }) : undefined
    error.value = body?.message ?? (caught as Error).message ?? 'The server refused that.'
  }
}

/**
 * Live tokens.
 *
 * `GET /v1/users/me/tokens` drops revoked ones entirely — measured, after a
 * revoke the list comes back `[]` rather than carrying a tombstone. The filter
 * stays anyway: `revoked_at` is part of the type, and if a server ever starts
 * returning revoked rows, rendering them as live would be the worse failure.
 */
const liveTokens = computed(() => (tokens.value ?? []).filter((token) => !token.revoked_at))

function expiryLabel(iso: string | null): string {
  if (!iso) return 'no expiry'
  return Date.parse(iso) < Date.now()
    ? `expired ${formatRelative(iso)}`
    : `expires ${formatRelative(iso)}`
}
</script>

<template>
  <div class="flex max-w-4xl flex-col gap-4">
    <UAlert
      v-if="error"
      color="warning"
      variant="subtle"
      icon="i-lucide-shield-alert"
      :description="error"
      role="alert"
      close
      @update:open="error = null"
    />

    <section class="rounded-xl border border-default p-4">
      <p class="cq-label mb-3">
        Signed in as
      </p>
      <dl class="grid max-w-sm grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-sm">
        <dt class="text-muted">
          Username
        </dt>
        <dd class="text-right font-mono">
          {{ me?.username ?? '—' }}
        </dd>
        <dt class="text-muted">
          Display name
        </dt>
        <dd class="text-right">
          {{ me?.display_name || '—' }}
        </dd>
        <dt class="text-muted">
          Email
        </dt>
        <dd class="text-right">
          {{ me?.email || '—' }}
        </dd>
        <dt class="text-muted">
          Role
        </dt>
        <dd class="text-right">
          {{ me?.role ?? '—' }}
        </dd>
        <dt class="text-muted">
          Last sign-in
        </dt>
        <dd class="text-right">
          {{ formatAbsolute(me?.last_login_at) }}
        </dd>
      </dl>
    </section>

    <section class="rounded-xl border border-default p-4">
      <div class="mb-3 flex items-center gap-3">
        <p class="cq-label">
          Two-factor authentication
        </p>
        <UBadge
          :color="me?.totp_enabled ? 'success' : 'neutral'"
          variant="subtle"
          size="sm"
        >
          {{ me?.totp_enabled ? 'on' : 'off' }}
        </UBadge>
      </div>

      <!-- Enrolment: the secret and the recovery codes, then a code to prove
           the authenticator actually took them. -->
      <div
        v-if="enrolment"
        class="flex max-w-2xl flex-col gap-4"
      >
        <SecretOnce
          title="Your authenticator secret"
          description="Scan it, or type it into your authenticator app. It is not shown again — if you lose it before confirming, start the enrolment over."
          :value="enrolment.secret"
          acknowledgement="I have added it to my authenticator"
        />
        <SecretOnce
          title="Recovery codes"
          description="Each one signs you in once if you lose your authenticator. Without them and without the app, an administrator has to reset your account."
          :value="enrolment.recovery_codes"
          acknowledgement="I have stored my recovery codes"
          @done="codesAcknowledged = true"
        />
        <UFormField
          label="Confirm"
          description="Enter the current six-digit code to finish enrolment."
        >
          <div class="flex items-center gap-2">
            <UInput
              v-model="code"
              class="w-40 font-mono"
              placeholder="000000"
              inputmode="numeric"
              aria-label="Six-digit code from your authenticator"
            />
            <UButton
              :loading="confirm.isPending.value"
              @click="confirmEnrolment"
            >
              Enable
            </UButton>
            <UButton
              color="neutral"
              variant="ghost"
              @click="enrolment = null"
            >
              Cancel
            </UButton>
          </div>
        </UFormField>
        <p
          v-if="!codesAcknowledged"
          class="text-xs text-muted"
        >
          Store the recovery codes before you finish — this page is the only
          place they appear.
        </p>
      </div>

      <!-- Removing the second factor takes the password, not a code. -->
      <div
        v-else-if="disabling"
        class="flex max-w-md flex-col gap-3"
      >
        <UFormField
          label="Password"
          description="Removing a second factor is a downgrade, so the server re-verifies your primary credential rather than the factor being removed."
        >
          <UInput
            v-model="disablePassword"
            type="password"
            class="w-full"
            autocomplete="current-password"
            aria-label="Your password"
          />
        </UFormField>
        <div class="flex gap-2">
          <UButton
            color="error"
            :loading="disableTotp.isPending.value"
            @click="doDisable"
          >
            Turn off 2FA
          </UButton>
          <UButton
            color="neutral"
            variant="ghost"
            @click="disabling = false"
          >
            Cancel
          </UButton>
        </div>
      </div>

      <div
        v-else
        class="flex items-center gap-2"
      >
        <UButton
          v-if="!me?.totp_enabled"
          icon="i-lucide-shield-check"
          :loading="setup.isPending.value"
          @click="beginEnrolment"
        >
          Set up 2FA
        </UButton>
        <UButton
          v-else
          icon="i-lucide-shield-off"
          color="neutral"
          variant="subtle"
          @click="disabling = true"
        >
          Turn off 2FA
        </UButton>
      </div>
    </section>

    <section class="rounded-xl border border-default p-4">
      <div class="mb-3 flex items-center justify-between gap-3">
        <p class="cq-label">
          Personal access tokens
        </p>
        <UButton
          v-if="!creatingToken && !mintedToken"
          icon="i-lucide-plus"
          color="neutral"
          variant="subtle"
          size="xs"
          @click="creatingToken = true"
        >
          New token
        </UButton>
      </div>

      <SecretOnce
        v-if="mintedToken"
        class="mb-4 max-w-2xl"
        :title="`Token “${mintedToken.name}”`"
        description="Use it as a bearer token. It is stored hashed, so this is the only time the value exists anywhere you can read it."
        :value="mintedToken.token"
        @done="mintedToken = null"
      />

      <div
        v-if="creatingToken"
        class="mb-4 flex max-w-2xl flex-col gap-4 rounded-lg border border-default p-4"
      >
        <div class="grid grid-cols-2 gap-4">
          <UFormField
            label="Name"
            description="How you will recognise it here."
            required
          >
            <UInput
              v-model="tokenName"
              class="w-full"
              placeholder="laptop-cli"
            />
          </UFormField>
          <UFormField
            label="Expires in (hours)"
            description="Empty = no expiry."
          >
            <UInput
              v-model="tokenExpiry"
              class="w-full font-mono"
              placeholder="720"
              inputmode="numeric"
            />
          </UFormField>
        </div>
        <ScopePicker v-model="tokenScopes" />
        <div class="flex justify-end gap-2">
          <UButton
            color="neutral"
            variant="ghost"
            size="xs"
            @click="creatingToken = false"
          >
            Cancel
          </UButton>
          <UButton
            size="xs"
            :loading="createPat.isPending.value"
            @click="createToken"
          >
            Create token
          </UButton>
        </div>
      </div>

      <AppLoading
        v-if="tokensPending"
        size="tight"
        label="Loading tokens"
      />
      <AppEmpty
        v-else-if="liveTokens.length === 0"
        size="tight"
        icon="i-lucide-key-round"
        title="No tokens"
        description="A personal access token acts as you, with the scopes you give it — for scripts and the CLI."
      />
      <table
        v-else
        class="w-full border-collapse"
      >
        <thead>
          <tr class="border-b border-default">
            <th class="cq-label py-[var(--cq-cell-y)] text-left">
              Token
            </th>
            <th class="cq-label py-[var(--cq-cell-y)] text-left">
              Scopes
            </th>
            <th class="cq-label py-[var(--cq-cell-y)] text-right">
              Last used
            </th>
            <th class="cq-label py-[var(--cq-cell-y)] text-right">
              Expiry
            </th>
            <th class="w-12" />
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="token in liveTokens"
            :key="token.token_id"
            class="cq-row border-b border-default/60"
          >
            <td class="max-w-[14rem] truncate">
              <span class="font-mono">{{ token.name }}</span>
              <span class="ml-2 font-mono text-xs text-muted">{{ token.token_prefix }}…</span>
            </td>
            <td class="max-w-[18rem] truncate font-mono text-xs text-muted">
              {{ token.scopes.join(' ') }}
            </td>
            <td
              class="cq-num text-right text-muted"
              :title="formatAbsolute(token.last_used_at)"
            >
              <!-- A token that has never been used is worth spotting: it is
                   either unnecessary or something is misconfigured. -->
              {{ token.last_used_at ? formatRelative(token.last_used_at) : 'never' }}
            </td>
            <td class="cq-num text-right text-muted">
              {{ expiryLabel(token.expires_at) }}
            </td>
            <td class="text-right">
              <UButton
                icon="i-lucide-trash-2"
                color="error"
                variant="ghost"
                size="xs"
                :aria-label="`Revoke ${token.name}`"
                title="Revoke"
                :loading="revokePat.isPending.value"
                @click="attempt(() => revokePat.mutateAsync(token.token_id))"
              />
            </td>
          </tr>
        </tbody>
      </table>
    </section>
  </div>
</template>
