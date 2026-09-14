<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute } from 'vue-router'
import { ApiError, apiPost } from '~/api/client'
import { checkPassword, PASSWORD_RULE } from '~/lib/password'

/**
 * Where a password-reset email lands.
 *
 * The server has been sending this link all along —
 * `{base}/password-reset/confirm?token=…`, built in
 * `api/password_reset.rs` — and no dashboard, in either tree, had a route for
 * it. Requesting a reset worked, the mail went out, and the link opened the
 * not-found page. The flow was half-built and looked complete from the side
 * that an operator tests.
 *
 * The token is in the query string because that is where the email put it. It
 * is single-use and expires in an hour; nothing here can tell whether it is
 * still good until it is spent, so the form does not pretend to.
 */
/** The server's own wording, when it sent any. */
function serverMessage(caught: unknown): string | null {
  if (!(caught instanceof ApiError)) return null
  return (caught.body as { message?: string } | undefined)?.message ?? null
}

const route = useRoute()
const token = computed(() => (route.query.token as string) || '')

const password = ref('')
const confirmation = ref('')
const pending = ref(false)
const error = ref<string | null>(null)
const done = ref(false)

async function submit() {
  error.value = null

  if (!token.value) {
    error.value = 'This link carries no reset token. Request a new one from the sign-in page.'
    return
  }
  const complaint = checkPassword(password.value)
  if (complaint) {
    error.value = complaint
    return
  }
  if (password.value !== confirmation.value) {
    error.value = 'The two passwords do not match.'
    return
  }

  pending.value = true
  try {
    await apiPost('/v1/auth/password-reset/confirm', {
      token: token.value,
      new_password: password.value,
    })
    done.value = true
  } catch (caught) {
    // Every status the handler can return, read off `password_reset.rs`
    // rather than guessed at. The first draft only covered 410 and 404 — and
    // an *unknown* token answers 401, which is the most likely failure of all,
    // so the commonest case fell through to a generic "refused".
    error.value = (() => {
      const status = caught instanceof ApiError ? caught.status : 0
      switch (status) {
        // 401: no such token. 410: used, or past its hour. Different causes,
        // and the same thing to do about them.
        case 401:
        case 410:
          return 'This link is no longer usable — reset links are good for one hour and one use. Request a new one from the sign-in page.'
        // The token resolved, the account behind it did not.
        case 404:
          return 'The account this link belongs to no longer exists.'
        case 403:
          return 'Password sign-in is disabled on this server, so a password cannot be set here.'
        case 400:
          return 'The server rejected that password. It must be 8–72 bytes.'
        case 503:
          return 'The server has no store configured and cannot set a password.'
        default:
          return serverMessage(caught) ?? 'The server refused that.'
      }
    })()
  } finally {
    pending.value = false
  }
}
</script>

<template>
  <div class="flex min-h-screen items-center justify-center p-6">
    <div class="w-full max-w-md">
      <BrandMark class="mb-6" />

      <div class="rounded-xl border border-default bg-default p-6 shadow-sm">
        <template v-if="done">
          <h1 class="text-lg font-semibold">
            Password set
          </h1>
          <p class="mt-1 text-sm text-muted">
            Sign in with the new one. The link you followed is spent.
          </p>
          <UButton
            to="/login"
            class="mt-5"
            block
            trailing-icon="i-lucide-arrow-right"
          >
            Go to sign in
          </UButton>
        </template>

        <form
          v-else
          class="flex flex-col gap-4"
          @submit.prevent="submit"
        >
          <div>
            <h1 class="text-lg font-semibold">
              Choose a new password
            </h1>
            <p class="mt-1 text-sm text-muted">
              This link is good for one hour and one use.
            </p>
          </div>

          <UAlert
            v-if="error"
            color="warning"
            variant="subtle"
            icon="i-lucide-shield-alert"
            :description="error"
            role="alert"
          />

          <UFormField
            label="New password"
            :description="PASSWORD_RULE"
            required
          >
            <UInput
              v-model="password"
              type="password"
              class="w-full"
              autocomplete="new-password"
              autofocus
            />
          </UFormField>

          <UFormField
            label="Repeat it"
            required
          >
            <UInput
              v-model="confirmation"
              type="password"
              class="w-full"
              autocomplete="new-password"
            />
          </UFormField>

          <UButton
            type="submit"
            :loading="pending"
            block
          >
            Set password
          </UButton>

          <RouterLink
            to="/login"
            class="text-center text-xs text-muted hover:text-default"
          >
            Back to sign in
          </RouterLink>
        </form>
      </div>
    </div>
  </div>
</template>
