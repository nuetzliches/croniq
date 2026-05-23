import { useState } from 'react'
import * as Dialog from '@radix-ui/react-dialog'
import { Plus, Trash2, X, Send } from 'lucide-react'
import {
  useUsers,
  useDeleteUser,
  useInvitations,
  useCreateInvitation,
  useRevokeInvitation,
  useCurrentUser,
} from '@/api/hooks'
import { EmptyState, StatusPill, Avatar, CopyBtn } from '@/components/primitives'
import { useConfirm } from '@/components/ui/confirm-dialog'
import { formatRelative } from '@/lib/utils'
import type { CreateInvitationResponse, Role } from '@/api/types'

export function UsersTab() {
  const { data: me } = useCurrentUser()

  if (!me) {
    return <EmptyState title="Profile unavailable" desc="The current session has no linked user." />
  }
  if (me.role !== 'admin') {
    return (
      <EmptyState
        title="Admin-only"
        desc="User and invitation management is restricted to admin accounts."
      />
    )
  }
  return <UsersTabAdmin />
}

function UsersTabAdmin() {
  const users = useUsers()
  const invitations = useInvitations()
  const deleteUser = useDeleteUser()
  const createInvitation = useCreateInvitation()
  const revokeInvitation = useRevokeInvitation()
  const { confirm, dialog: confirmDialog } = useConfirm()
  const [inviteOpen, setInviteOpen] = useState(false)
  const [email, setEmail] = useState('')
  const [role, setRole] = useState<Role>('viewer')
  const [issued, setIssued] = useState<CreateInvitationResponse | null>(null)

  async function invite() {
    if (!email.trim()) return
    try {
      const res = await createInvitation.mutateAsync({ email: email.trim(), role, expires_in_hours: 72 })
      setIssued(res)
      setEmail('')
    } catch {
      /* mutation toast */
    }
  }

  async function deleteByConfirm(id: string, label: string) {
    const ok = await confirm({
      title: `Delete user ${label}?`,
      description: 'The user can no longer log in. Past audit events and executions are kept.',
      confirmLabel: 'Delete user',
      destructive: true,
    })
    if (ok) deleteUser.mutate(id)
  }

  async function revokeInvite(id: string, mail: string) {
    const ok = await confirm({
      title: `Revoke invitation for ${mail}?`,
      description: 'The accept link will return 410 the next time someone follows it.',
      confirmLabel: 'Revoke invitation',
      destructive: true,
    })
    if (ok) revokeInvitation.mutate(id)
  }

  return (
    <div className="col" style={{ gap: 14 }}>
      {confirmDialog}

      <section className="card" style={{ padding: 0 }}>
        <div className="row between" style={{ padding: 16 }}>
          <p className="card-title">Users</p>
          <Dialog.Root open={inviteOpen} onOpenChange={(o) => {
            setInviteOpen(o)
            if (!o) setIssued(null)
          }}>
            <Dialog.Trigger asChild>
              <button type="button" className="btn sm primary">
                <Plus size={12} /> Invite user
              </button>
            </Dialog.Trigger>
            <Dialog.Portal>
              <Dialog.Overlay className="modal-backdrop" />
              <Dialog.Content className="modal">
                <div className="modal-head">
                  <Dialog.Title className="modal-title">Invite a user</Dialog.Title>
                  <Dialog.Close className="btn icon sm ghost" aria-label="Close">
                    <X size={14} />
                  </Dialog.Close>
                </div>
                <div className="modal-body col" style={{ gap: 10 }}>
                  {issued ? (
                    <>
                      <div className="banner info" role="status">
                        <span className="grow">Share this accept URL with {issued.email}. It expires when the invitation does.</span>
                      </div>
                      <div className="row" style={{ gap: 8 }}>
                        <code
                          className="mono"
                          style={{
                            fontSize: 11.5,
                            padding: '6px 8px',
                            background: 'var(--bg-2)',
                            border: '1px solid var(--border)',
                            borderRadius: 'var(--r-2)',
                            flex: 1,
                            overflow: 'hidden',
                            textOverflow: 'ellipsis',
                            whiteSpace: 'nowrap',
                          }}
                        >
                          {issued.accept_url}
                        </code>
                        <CopyBtn value={issued.accept_url} label="Copy" />
                      </div>
                    </>
                  ) : (
                    <>
                      <label className="col" style={{ gap: 4 }}>
                        <span style={{ fontSize: 12, color: 'var(--fg-2)' }}>Email</span>
                        <input
                          className="input"
                          type="email"
                          value={email}
                          onChange={(e) => setEmail(e.target.value)}
                          autoFocus
                        />
                      </label>
                      <label className="col" style={{ gap: 4 }}>
                        <span style={{ fontSize: 12, color: 'var(--fg-2)' }}>Role</span>
                        <select className="input" value={role} onChange={(e) => setRole(e.target.value as Role)}>
                          <option value="viewer">Viewer</option>
                          <option value="operator">Operator</option>
                          <option value="admin">Admin</option>
                        </select>
                      </label>
                    </>
                  )}
                </div>
                <div className="modal-foot">
                  {issued ? (
                    <button type="button" className="btn primary" onClick={() => setInviteOpen(false)}>
                      Done
                    </button>
                  ) : (
                    <>
                      <Dialog.Close className="btn ghost">Cancel</Dialog.Close>
                      <button
                        type="button"
                        className="btn primary"
                        onClick={invite}
                        disabled={!email.trim() || createInvitation.isPending}
                      >
                        <Send size={13} /> Send invitation
                      </button>
                    </>
                  )}
                </div>
              </Dialog.Content>
            </Dialog.Portal>
          </Dialog.Root>
        </div>
        <div style={{ borderTop: '1px solid var(--divider)' }}>
          {users.isLoading ? (
            <div className="dim center" style={{ padding: 30 }}>Loading…</div>
          ) : (users.data ?? []).length === 0 ? (
            <EmptyState title="No users" desc="Invite teammates to collaborate." />
          ) : (
            <table className="tbl">
              <thead>
                <tr>
                  <th></th>
                  <th>Username</th>
                  <th>Role</th>
                  <th>Email</th>
                  <th>Last login</th>
                  <th>Status</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {(users.data ?? []).map((u) => (
                  <tr key={u.user_id}>
                    <td><Avatar name={u.display_name || u.username} size="sm" /></td>
                    <td>{u.username}</td>
                    <td><span className="pill accent" style={{ fontFamily: 'var(--font-mono-app)' }}>{u.role}</span></td>
                    <td className="dim">{u.email ?? '—'}</td>
                    <td className="dim">{u.last_login_at ? formatRelative(u.last_login_at) : 'never'}</td>
                    <td><StatusPill state={u.is_active ? 'active' : 'disabled'} /></td>
                    <td style={{ textAlign: 'right' }}>
                      <button
                        type="button"
                        className="btn icon sm danger-hover"
                        onClick={() => deleteByConfirm(u.user_id, u.username)}
                        aria-label={`Delete ${u.username}`}
                      >
                        <Trash2 size={12} />
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </section>

      <section className="card" style={{ padding: 0 }}>
        <div className="row between" style={{ padding: 16 }}>
          <p className="card-title">Pending invitations</p>
        </div>
        <div style={{ borderTop: '1px solid var(--divider)' }}>
          {invitations.isLoading ? (
            <div className="dim center" style={{ padding: 30 }}>Loading…</div>
          ) : (invitations.data ?? []).filter((i) => !i.accepted_at && !i.revoked_at).length === 0 ? (
            <EmptyState title="No pending invitations" desc="Invite a user above and share the accept link." />
          ) : (
            <table className="tbl">
              <thead>
                <tr>
                  <th>Email</th>
                  <th>Role</th>
                  <th>Expires</th>
                  <th>Invited</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {(invitations.data ?? [])
                  .filter((i) => !i.accepted_at && !i.revoked_at)
                  .map((inv) => (
                    <tr key={inv.invitation_id}>
                      <td>{inv.email}</td>
                      <td>
                        <span className="pill accent" style={{ fontFamily: 'var(--font-mono-app)' }}>{inv.role}</span>
                      </td>
                      <td className="dim">{formatRelative(inv.expires_at)}</td>
                      <td className="dim">{formatRelative(inv.created_at)}</td>
                      <td style={{ textAlign: 'right' }}>
                        <button
                          type="button"
                          className="btn sm ghost"
                          onClick={() => revokeInvite(inv.invitation_id, inv.email)}
                        >
                          Revoke
                        </button>
                      </td>
                    </tr>
                  ))}
              </tbody>
            </table>
          )}
        </div>
      </section>
    </div>
  )
}
