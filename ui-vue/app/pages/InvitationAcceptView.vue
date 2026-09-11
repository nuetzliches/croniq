<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute } from 'vue-router'
import { ApiError, apiPost } from '~/api/client'
import { checkPassword, PASSWORD_RULE } from '~/lib/password'

/**
 * Where an invitation email lands.
 *
 * Same hole as the password reset, and found the same way: the server builds
 * `{base}/invitations/accept?token=…` in `api/invitations.rs`, the settings
 * screen hands that link to an admin to send on, and until now it opened the
 * not-found page. Someone invited to a Croniq server could not get in.
 *
 * The invitation carries the role; this form only asks for the two things the
 * server needs and cannot know — a username and a password.
 */
/** The server's own wording, when it sent any. */
function serverMessage(caught: unknown): string | null {
  if (!(caught instanceof ApiError)) return null
  return (caught.body as { message?: string } | undefined)?.message ?? null
}

const route = useRoute()
const token = computed(() => (route.query.token as string) || '')

const username = ref('')
const password = ref('')
const confirmation = ref('')
const pending = ref(false)
const error = ref<string | null>(null)
const done = ref(false)

async function submit() {
  error.value = null

  if (!token.value) {
    error.value = 'This link carries no invitation token. Ask for a new invitation.'
    return
  }
  if (!username.value.trim()) {
    error.value = 'Choose a username — it is what you will sign in with.'
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
    await apiPost('/v1/invitations/accept', {
      token: token.value,
      username: username.value.trim(),
      password: password.value,
    })
    done.value = true
  } catch (caught) {
    // Read off `invitations.rs`, every branch it can take.
    error.value = (() => {
      const status = caught instanceof ApiError ? caught.status : 0
      switch (status) {
        // 401: no such token — including one already spent, since accepting
        // consumes it. 410: revoked, or past its expiry.
        case 401:
        case 410:
          return 'This invitation is no longer usable — it may have expired, been revoked, or already been accepted. Ask an administrator for a new one.'
        case 409:
          return 'That username is taken. Pick another.'
        case 400:
          return 'The server rejected the username or password. A password must be 8–72 bytes.'
        case 503:
          return 'The server has no store configured and cannot create an account.'
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
            Account created
          </h1>
          <p class="mt-1 text-sm text-muted">
            Sign in with the username and password you just chose.
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
              Accept your invitation
            </h1>
            <p class="mt-1 text-sm text-muted">
              Choose how you will sign in. Your role comes with the invitation.
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
            label="Username"
            description="What you will type to sign in."
            required
          >
            <UInput
              v-model="username"
              class="w-full font-mono"
              autocomplete="username"
              autofocus
            />
          </UFormField>

          <UFormField
            label="Password"
            :description="PASSWORD_RULE"
            required
          >
            <UInput
              v-model="password"
              type="password"
              class="w-full"
              autocomplete="new-password"
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
            Create my account
          </UButton>
        </form>
      </div>
    </div>
  </div>
</template>
