<script setup lang="ts">
import { computed, ref } from 'vue'
import { ApiError } from '~/api/client'
import {
  useApiClients,
  useCreateApiClient,
  useDeleteApiClient,
  useIssueClientToken,
  useUpdateApiClient,
} from '~/api/queries'
import type { ApiClient } from '~/api/types'
import { declaringKeyVar, MANAGED_BY_ENV } from '~/lib/env-managed'
import { formatAbsolute } from '~/lib/format'

/**
 * Machine access: the clients, their scopes, and minting a key.
 *
 * The thing this screen has to get right is the environment-managed client.
 * The server refuses every mutation on one, and the dashboard has to say so
 * *before* the attempt — naming the variable to edit instead. That name is not
 * a plain template over the client name: `default` is declared by
 * `CRONIQ_API_KEY`, outside the `CRONIQ_API_CLIENT_` namespace, and the
 * templated guess would have an operator add a second declaration of the same
 * client, which the server refuses at its next boot (issue #481). See
 * `lib/env-managed.ts`, which is tested for exactly that case.
 */
const { data: clients, isPending } = useApiClients()

const createClient = useCreateApiClient()
const updateClient = useUpdateApiClient()
const deleteClient = useDeleteApiClient()
const issueKey = useIssueClientToken()

const error = ref<string | null>(null)
const mintedKey = ref<{ client: string; key: string } | null>(null)

const creating = ref(false)
const name = ref('')
const scopes = ref<string[]>([])

/** The client being edited, by id. */
const editing = ref<string | null>(null)
const editScopes = ref<string[]>([])

const rows = computed(() =>
  [...(clients.value ?? [])].sort((a, b) => a.name.localeCompare(b.name)),
)

const envManaged = (client: ApiClient) => client.managed_by === MANAGED_BY_ENV

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

async function create() {
  if (!name.value.trim()) {
    error.value = 'A client needs a name.'
    return
  }
  if (scopes.value.length === 0) {
    error.value = 'A client with no scopes can do nothing. Pick at least one.'
    return
  }
  const ok = await attempt(() =>
    createClient.mutateAsync({ name: name.value.trim(), scopes: [...scopes.value] }),
  )
  if (ok) {
    creating.value = false
    name.value = ''
    scopes.value = []
  }
}

async function mint(client: ApiClient) {
  error.value = null
  try {
    const created = await issueKey.mutateAsync(client.client_id)
    mintedKey.value = { client: client.name, key: created.raw_key }
  } catch (caught) {
    const body = caught instanceof ApiError ? (caught.body as { message?: string }) : undefined
    error.value = body?.message ?? (caught as Error).message ?? 'The server refused that.'
  }
}

function startEdit(client: ApiClient) {
  editing.value = client.client_id
  editScopes.value = [...client.scopes]
  error.value = null
}

async function saveScopes(client: ApiClient) {
  const ok = await attempt(() =>
    updateClient.mutateAsync({ client_id: client.client_id, scopes: [...editScopes.value] }),
  )
  if (ok) editing.value = null
}

function toggleActive(client: ApiClient) {
  void attempt(() =>
    updateClient.mutateAsync({ client_id: client.client_id, is_active: !client.is_active }),
  )
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

    <SecretOnce
      v-if="mintedKey"
      class="max-w-2xl"
      :title="`API key for “${mintedKey.client}”`"
      description="Give it to the client as its bearer credential. The server stores only a hash, so this is the only time the value is readable."
      :value="mintedKey.key"
      @done="mintedKey = null"
    />

    <div
      v-if="creating"
      class="flex max-w-2xl flex-col gap-4 rounded-lg border border-default p-4"
    >
      <UFormField
        label="Name"
        description="Identifies the client in the audit log and in key listings."
        required
      >
        <UInput
          v-model="name"
          class="w-full font-mono"
          placeholder="ci-pipeline"
        />
      </UFormField>
      <ScopePicker v-model="scopes" />
      <div class="flex justify-end gap-2">
        <UButton
          color="neutral"
          variant="ghost"
          size="xs"
          @click="creating = false"
        >
          Cancel
        </UButton>
        <UButton
          size="xs"
          :loading="createClient.isPending.value"
          @click="create"
        >
          Create client
        </UButton>
      </div>
    </div>

    <div class="flex items-center justify-between gap-3">
      <p class="text-sm text-muted">
        Machines and scripts that hold a key, and what each one is allowed to do.
      </p>
      <UButton
        v-if="!creating"
        icon="i-lucide-plus"
        size="sm"
        @click="creating = true"
      >
        New client
      </UButton>
    </div>

    <AppLoading
      v-if="isPending"
      size="tight"
      label="Loading clients"
    />
    <AppEmpty
      v-else-if="rows.length === 0"
      size="tight"
      icon="i-lucide-plug"
      title="No API clients"
      description="A client holds a key and a set of scopes. Runners use one; so does anything calling the API without a user session."
    />

    <ul
      v-else
      class="flex flex-col gap-2"
    >
      <li
        v-for="client in rows"
        :key="client.client_id"
        class="rounded-lg border border-default p-3"
        :class="!client.is_active && 'opacity-60'"
      >
        <div class="flex flex-wrap items-center gap-2">
          <span class="font-mono text-sm">{{ client.name }}</span>
          <UBadge
            v-if="envManaged(client)"
            color="neutral"
            variant="subtle"
            size="sm"
            :title="`Declared by ${declaringKeyVar(client.name)} in the server's environment`"
          >
            environment
          </UBadge>
          <UBadge
            v-if="!client.is_active"
            color="warning"
            variant="subtle"
            size="sm"
          >
            inactive
          </UBadge>
          <span
            class="cq-num text-xs text-muted"
            :title="formatAbsolute(client.created_at)"
          >created {{ formatAbsolute(client.created_at) }}</span>

          <div class="ml-auto flex items-center gap-1">
            <UButton
              icon="i-lucide-key-round"
              color="neutral"
              variant="subtle"
              size="xs"
              :disabled="envManaged(client)"
              :title="
                envManaged(client)
                  ? `Its key comes from ${declaringKeyVar(client.name)}; the server refuses to mint another`
                  : 'Mint a new key for this client'
              "
              :loading="issueKey.isPending.value"
              @click="mint(client)"
            >
              New key
            </UButton>
            <UButton
              icon="i-lucide-pencil"
              color="neutral"
              variant="ghost"
              size="xs"
              :disabled="envManaged(client)"
              :aria-label="`Edit the scopes of ${client.name}`"
              :title="envManaged(client) ? `Edit ${declaringKeyVar(client.name)} instead` : 'Edit scopes'"
              @click="startEdit(client)"
            />
            <UButton
              :icon="client.is_active ? 'i-lucide-pause' : 'i-lucide-play'"
              color="neutral"
              variant="ghost"
              size="xs"
              :disabled="envManaged(client)"
              :aria-label="`${client.is_active ? 'Deactivate' : 'Activate'} ${client.name}`"
              :loading="updateClient.isPending.value"
              @click="toggleActive(client)"
            />
            <UButton
              icon="i-lucide-trash-2"
              color="error"
              variant="ghost"
              size="xs"
              :disabled="envManaged(client)"
              :aria-label="`Delete ${client.name}`"
              :title="
                envManaged(client)
                  ? `Remove ${declaringKeyVar(client.name)} from the environment instead`
                  : 'Delete'
              "
              :loading="deleteClient.isPending.value"
              @click="attempt(() => deleteClient.mutateAsync(client.client_id))"
            />
          </div>
        </div>

        <!-- Named in advance, not after a refusal: the point of saying it here
             is that the operator never makes the attempt. -->
        <p
          v-if="envManaged(client)"
          class="mt-2 text-xs text-muted"
        >
          The environment owns this client. Edit
          <code class="font-mono">{{ declaringKeyVar(client.name) }}</code> and restart the
          server; changes made here would be refused.
        </p>

        <div
          v-if="editing === client.client_id"
          class="mt-3 flex flex-col gap-3"
        >
          <ScopePicker v-model="editScopes" />
          <div class="flex justify-end gap-2">
            <UButton
              color="neutral"
              variant="ghost"
              size="xs"
              @click="editing = null"
            >
              Cancel
            </UButton>
            <UButton
              size="xs"
              :loading="updateClient.isPending.value"
              @click="saveScopes(client)"
            >
              Save scopes
            </UButton>
          </div>
        </div>
        <p
          v-else
          class="mt-2 font-mono text-xs text-muted"
        >
          {{ client.scopes.join(' ') || 'no scopes' }}
        </p>
      </li>
    </ul>
  </div>
</template>
