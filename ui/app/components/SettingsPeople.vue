<script setup lang="ts">
import { computed, ref } from 'vue'
import { ApiError } from '~/api/client'
import {
  useCreateInvitation,
  useCurrentUser,
  useDeleteUser,
  useInvitations,
  useRevokeInvitation,
  useUsers,
} from '~/api/queries'
import type { Invitation, Role, User } from '~/api/types'
import { formatAbsolute, formatRelative } from '~/lib/format'

/**
 * Who has access.
 *
 * The React tree rendered two tables, users and invitations, one above the
 * other. They are two answers to one question — an invitation is a person who
 * has been given access and has not arrived yet — and splitting them meant
 * that "who can sign in to this server" was something you assembled in your
 * head from two lists with different columns.
 *
 * One list, with a status. The actions differ per row (revoke an invitation,
 * remove a user) and that is fine; they are on the row.
 */
const { data: users, isPending: usersPending } = useUsers()
const { data: invitations, isPending: invitationsPending } = useInvitations()
const { data: me } = useCurrentUser()

const invite = useCreateInvitation()
const revokeInvitation = useRevokeInvitation()
const removeUser = useDeleteUser()

const error = ref<string | null>(null)

/** The accept link, shown once — with no SMTP configured it is the only copy. */
const issuedInvite = ref<{ email: string; url: string } | null>(null)

const inviting = ref(false)
const inviteEmail = ref('')
const inviteRole = ref<Role>('viewer')
const inviteHours = ref('')

type Row =
  | { kind: 'user'; id: string; identity: string; role: Role; status: string; tone: string; when: string | null; whenLabel: string; user: User }
  | { kind: 'invitation'; id: string; identity: string; role: Role; status: string; tone: string; when: string | null; whenLabel: string; invitation: Invitation }

/**
 * An invitation's state is a small decision table, and getting it wrong makes
 * a revoked invitation look like a pending one — i.e. makes it look as though
 * somebody still has a way in.
 */
function invitationStatus(invitation: Invitation): { status: string; tone: string } {
  if (invitation.revoked_at) return { status: 'revoked', tone: 'text-muted' }
  if (invitation.accepted_at) return { status: 'accepted', tone: 'text-muted' }
  if (Date.parse(invitation.expires_at) < Date.now())
    return { status: 'expired', tone: 'text-muted' }
  return { status: 'invited', tone: 'text-warning' }
}

/**
 * Which invitations belong on a list titled "who has access".
 *
 * Accepted ones are users now, and showing both would count the same person
 * twice. Revoked ones are a decision someone already made and carried out —
 * the row is noise that accumulates forever, and the audit log is where that
 * history belongs.
 *
 * Expired ones stay, and the distinction is the point: an expired invitation
 * is not a closed matter, it is somebody still waiting to be let in. "Why
 * hasn't she signed in yet" is answered by seeing it here, and the answer is
 * to invite her again.
 */
function stillOpen(invitation: Invitation): boolean {
  return !invitation.accepted_at && !invitation.revoked_at
}

const rows = computed<Row[]>(() => {
  const userRows: Row[] = (users.value ?? []).map((user) => ({
    kind: 'user',
    id: user.user_id,
    identity: user.username,
    role: user.role,
    status: user.is_active ? 'active' : 'deactivated',
    tone: user.is_active ? 'text-success' : 'text-muted',
    when: user.last_login_at,
    whenLabel: user.last_login_at ? formatRelative(user.last_login_at) : 'never signed in',
    user,
  }))

  const inviteRows: Row[] = (invitations.value ?? [])
    .filter(stillOpen)
    .map((invitation) => {
      const { status, tone } = invitationStatus(invitation)
      return {
        kind: 'invitation' as const,
        id: invitation.invitation_id,
        identity: invitation.email,
        role: invitation.role,
        status,
        tone,
        when: invitation.expires_at,
        whenLabel:
          status === 'invited'
            ? `expires ${formatRelative(invitation.expires_at)}`
            : formatRelative(invitation.created_at),
        invitation,
      }
    })

  return [...userRows, ...inviteRows].sort((a, b) => a.identity.localeCompare(b.identity))
})

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

async function sendInvite() {
  if (!inviteEmail.value.trim()) {
    error.value = 'An invitation needs an email address.'
    return
  }
  const hours = inviteHours.value.trim() ? Number(inviteHours.value) : undefined
  if (inviteHours.value.trim() && (!Number.isFinite(hours) || hours! <= 0)) {
    error.value = 'Validity is a number of hours, or empty for the server default.'
    return
  }
  error.value = null
  try {
    const created = await invite.mutateAsync({
      email: inviteEmail.value.trim(),
      role: inviteRole.value,
      expires_in_hours: hours,
    })
    issuedInvite.value = { email: created.email, url: created.accept_url }
    inviting.value = false
    inviteEmail.value = ''
    inviteHours.value = ''
  } catch (caught) {
    const body = caught instanceof ApiError ? (caught.body as { message?: string }) : undefined
    error.value = body?.message ?? (caught as Error).message ?? 'The server refused that.'
  }
}

const ROLES: Role[] = ['admin', 'operator', 'viewer']

/** Removing yourself is the one mistake this screen can make irreversible. */
function isSelf(row: Row): boolean {
  return row.kind === 'user' && row.user.user_id === me.value?.user_id
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
      v-if="issuedInvite"
      class="max-w-2xl"
      :title="`Invitation for ${issuedInvite.email}`"
      description="Send this link to them yourself. With no SMTP transport configured the server does not email it, and it is not retrievable afterwards."
      :value="issuedInvite.url"
      acknowledgement="I have sent the link"
      @done="issuedInvite = null"
    />

    <div
      v-if="inviting"
      class="flex max-w-2xl flex-wrap items-end gap-3 rounded-lg border border-default p-4"
    >
      <UFormField
        label="Email"
        required
        class="flex-1"
      >
        <UInput
          v-model="inviteEmail"
          class="w-full"
          type="email"
          placeholder="someone@example.com"
        />
      </UFormField>
      <UFormField label="Role">
        <USelectMenu
          v-model="inviteRole"
          :items="ROLES"
          class="w-36"
          aria-label="Role for the invited person"
        />
      </UFormField>
      <UFormField
        label="Valid for (hours)"
        description="Empty = server default."
      >
        <UInput
          v-model="inviteHours"
          class="w-32 font-mono"
          placeholder="72"
          inputmode="numeric"
        />
      </UFormField>
      <div class="flex gap-2">
        <UButton
          color="neutral"
          variant="ghost"
          @click="inviting = false"
        >
          Cancel
        </UButton>
        <UButton
          :loading="invite.isPending.value"
          @click="sendInvite"
        >
          Invite
        </UButton>
      </div>
    </div>

    <div class="flex items-center justify-between gap-3">
      <p class="text-sm text-muted">
        Everyone who can sign in, and everyone who has been asked to.
      </p>
      <UButton
        v-if="!inviting"
        icon="i-lucide-user-plus"
        size="sm"
        @click="inviting = true"
      >
        Invite someone
      </UButton>
    </div>

    <AppLoading
      v-if="usersPending || invitationsPending"
      size="tight"
      label="Loading people"
    />
    <AppEmpty
      v-else-if="rows.length === 0"
      size="tight"
      icon="i-lucide-users"
      title="Nobody yet"
      description="Not even you — which means this session is an API-key session rather than a user one."
    />
    <table
      v-else
      class="w-full border-collapse"
    >
      <thead>
        <tr class="border-b border-default">
          <th class="cq-label py-[var(--cq-cell-y)] text-left">
            Person
          </th>
          <th class="cq-label py-[var(--cq-cell-y)] text-left">
            Role
          </th>
          <th class="cq-label py-[var(--cq-cell-y)] text-left">
            Status
          </th>
          <th class="cq-label py-[var(--cq-cell-y)] text-right">
            Activity
          </th>
          <th class="w-12" />
        </tr>
      </thead>
      <tbody>
        <tr
          v-for="row in rows"
          :key="`${row.kind}:${row.id}`"
          class="cq-row border-b border-default/60"
        >
          <td class="max-w-[18rem] truncate">
            <span class="font-mono">{{ row.identity }}</span>
            <span
              v-if="isSelf(row)"
              class="ml-2 text-xs text-muted"
            >you</span>
            <span
              v-else-if="row.kind === 'user' && row.user.email"
              class="ml-2 text-xs text-muted"
            >{{ row.user.email }}</span>
          </td>
          <td>{{ row.role }}</td>
          <td :class="row.tone">
            {{ row.status }}
          </td>
          <td
            class="cq-num text-right text-muted"
            :title="formatAbsolute(row.when)"
          >
            {{ row.whenLabel }}
          </td>
          <td class="text-right">
            <UButton
              v-if="row.kind === 'invitation' && row.status === 'invited'"
              icon="i-lucide-x"
              color="error"
              variant="ghost"
              size="xs"
              :aria-label="`Revoke the invitation for ${row.identity}`"
              title="Revoke the invitation"
              :loading="revokeInvitation.isPending.value"
              @click="attempt(() => revokeInvitation.mutateAsync(row.id))"
            />
            <UButton
              v-else-if="row.kind === 'user' && !isSelf(row)"
              icon="i-lucide-trash-2"
              color="error"
              variant="ghost"
              size="xs"
              :aria-label="`Remove ${row.identity}`"
              title="Remove this user"
              :loading="removeUser.isPending.value"
              @click="attempt(() => removeUser.mutateAsync(row.id))"
            />
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
