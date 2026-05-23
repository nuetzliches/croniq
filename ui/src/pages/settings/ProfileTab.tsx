import { useState } from 'react'
import * as Dialog from '@radix-ui/react-dialog'
import { Plus, Trash2, ShieldCheck, ShieldOff, X } from 'lucide-react'
import {
  useCurrentUser,
  usePersonalAccessTokens,
  useCreatePat,
  useRevokePat,
  useTotpSetup,
  useTotpConfirm,
  useTotpDisable,
} from '@/api/hooks'
import { BrandMark, EmptyState, StatusPill, CopyBtn } from '@/components/primitives'
import { useConfirm } from '@/components/ui/confirm-dialog'
import { formatRelative } from '@/lib/utils'
import type { CreatePatResponse, TotpSetupResponse } from '@/api/types'

export function ProfileTab() {
  const { data: me, isLoading } = useCurrentUser()

  if (isLoading) {
    return <div className="dim center" style={{ padding: 40 }}>Loading…</div>
  }
  if (!me) {
    return <EmptyState title="Profile unavailable" desc="The current session has no linked user record (e.g. API-key login)." />
  }

  return (
    <div className="col" style={{ gap: 14 }}>
      <section className="card">
        <div className="card-head">
          <p className="card-title">Identity</p>
          <StatusPill state={me.is_active ? 'active' : 'disabled'} />
        </div>
        <div className="col" style={{ gap: 8 }}>
          <InfoRow label="Username" value={me.username} />
          <InfoRow label="Email" value={me.email ?? '—'} />
          <InfoRow label="Display name" value={me.display_name ?? '—'} />
          <InfoRow label="Role" value={<span className="pill accent" style={{ fontFamily: 'var(--font-mono-app)' }}>{me.role}</span>} />
          <InfoRow label="Last login" value={me.last_login_at ? formatRelative(me.last_login_at) : '—'} />
        </div>
      </section>

      <TotpSection />
      <PatSection />
    </div>
  )
}

function TotpSection() {
  const setup = useTotpSetup()
  const confirm = useTotpConfirm()
  const disable = useTotpDisable()
  const [setupData, setSetupData] = useState<TotpSetupResponse | null>(null)
  const [code, setCode] = useState('')
  const [disableCode, setDisableCode] = useState('')
  const [savedAck, setSavedAck] = useState(false)
  const [error, setError] = useState('')

  async function startSetup() {
    setError('')
    try {
      const res = await setup.mutateAsync()
      setSetupData(res)
      setCode('')
      setSavedAck(false)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Setup failed')
    }
  }

  async function confirmSetup() {
    setError('')
    try {
      await confirm.mutateAsync(code.trim())
      setSetupData(null)
      setCode('')
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Wrong code')
    }
  }

  async function disableTotp() {
    setError('')
    try {
      await disable.mutateAsync(disableCode.trim())
      setDisableCode('')
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Disable failed')
    }
  }

  return (
    <section className="card">
      <div className="card-head">
        <p className="card-title">Two-factor authentication</p>
      </div>

      {setupData ? (
        <div className="col" style={{ gap: 10 }}>
          <p className="dim" style={{ margin: 0, fontSize: 12.5 }}>
            Scan this URL with your authenticator app, then confirm with a 6-digit code.
          </p>
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
                minWidth: 0,
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
              }}
            >
              {setupData.otpauth_url}
            </code>
            <CopyBtn value={setupData.otpauth_url} />
          </div>

          <div className="banner warn" role="status" style={{ flexDirection: 'column', alignItems: 'flex-start', gap: 6 }}>
            <span className="grow"><strong>Save these recovery codes.</strong> They're shown once.</span>
            <ul className="mono" style={{ margin: 0, paddingLeft: 18, columns: 2, gap: 4, fontSize: 12 }}>
              {setupData.recovery_codes.map((c, i) => (
                <li key={i}>{c}</li>
              ))}
            </ul>
            <label className="row" style={{ gap: 6, fontSize: 12, marginTop: 4 }}>
              <input type="checkbox" checked={savedAck} onChange={(e) => setSavedAck(e.target.checked)} />
              I have saved these codes
            </label>
          </div>

          <div className="row" style={{ gap: 6 }}>
            <input
              className="input mono"
              placeholder="000000"
              value={code}
              onChange={(e) => setCode(e.target.value)}
              maxLength={6}
              inputMode="numeric"
              style={{ letterSpacing: '0.3em', textAlign: 'center', maxWidth: 160 }}
            />
            <button
              type="button"
              className="btn primary"
              disabled={!savedAck || code.length !== 6 || confirm.isPending}
              onClick={confirmSetup}
            >
              {confirm.isPending ? <BrandMark spinning size={13} /> : <ShieldCheck size={13} />} Enable
            </button>
          </div>
          {error ? <p className="error" style={{ color: 'var(--error)', fontSize: 12, margin: 0 }}>{error}</p> : null}
        </div>
      ) : (
        <div className="col" style={{ gap: 10 }}>
          <p className="dim" style={{ margin: 0, fontSize: 12.5 }}>
            Enable a TOTP authenticator app to require a second factor at login.
          </p>
          <div className="row" style={{ gap: 6, flexWrap: 'wrap' }}>
            <button type="button" className="btn primary" onClick={startSetup} disabled={setup.isPending}>
              {setup.isPending ? <BrandMark spinning size={13} /> : <ShieldCheck size={13} />} Begin setup
            </button>
            <div className="row" style={{ gap: 6, marginLeft: 'auto' }}>
              <input
                className="input mono"
                placeholder="Disable code"
                value={disableCode}
                onChange={(e) => setDisableCode(e.target.value)}
                maxLength={6}
                inputMode="numeric"
                style={{ letterSpacing: '0.3em', textAlign: 'center', maxWidth: 140 }}
              />
              <button
                type="button"
                className="btn danger"
                disabled={disableCode.length !== 6 || disable.isPending}
                onClick={disableTotp}
                title="Disable TOTP (requires current 6-digit code)"
              >
                {disable.isPending ? <BrandMark spinning size={13} /> : <ShieldOff size={13} />} Disable
              </button>
            </div>
          </div>
          {error ? <p style={{ color: 'var(--error)', fontSize: 12, margin: 0 }}>{error}</p> : null}
        </div>
      )}
    </section>
  )
}

function PatSection() {
  const tokens = usePersonalAccessTokens()
  const createPat = useCreatePat()
  const revokePat = useRevokePat()
  const [dialogOpen, setDialogOpen] = useState(false)
  const [name, setName] = useState('')
  const [issuedSecret, setIssuedSecret] = useState<CreatePatResponse | null>(null)
  const { confirm, dialog: confirmDialog } = useConfirm()

  async function issuePat() {
    if (!name.trim()) return
    try {
      const res = await createPat.mutateAsync({
        name: name.trim(),
        scopes: ['admin'],
        expires_in_hours: 24 * 90,
      })
      setIssuedSecret(res)
      setName('')
    } catch {
      /* surfaced via mutation toast */
    }
  }

  async function revoke(id: string, label: string) {
    const ok = await confirm({
      title: `Revoke token "${label}"?`,
      description: 'Any client using this token will start receiving 401 immediately.',
      confirmLabel: 'Revoke token',
      destructive: true,
    })
    if (ok) revokePat.mutate(id)
  }

  return (
    <section className="card" style={{ padding: 0 }}>
      {confirmDialog}
      <div className="row between" style={{ padding: 16 }}>
        <p className="card-title">Personal Access Tokens</p>
        <Dialog.Root open={dialogOpen} onOpenChange={setDialogOpen}>
          <Dialog.Trigger asChild>
            <button type="button" className="btn sm primary">
              <Plus size={12} /> New token
            </button>
          </Dialog.Trigger>
          <Dialog.Portal>
            <Dialog.Overlay className="modal-backdrop" />
            <Dialog.Content className="modal">
              <div className="modal-head">
                <Dialog.Title className="modal-title">Create personal access token</Dialog.Title>
                <Dialog.Close className="btn icon sm ghost" aria-label="Close">
                  <X size={14} />
                </Dialog.Close>
              </div>
              <div className="modal-body col" style={{ gap: 10 }}>
                {issuedSecret ? (
                  <>
                    <div className="banner warn" role="status">
                      <span className="grow">Copy this token now — it's shown only once.</span>
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
                        {issuedSecret.token}
                      </code>
                      <CopyBtn value={issuedSecret.token} label="Copy" />
                    </div>
                  </>
                ) : (
                  <>
                    <label className="col" style={{ gap: 4 }}>
                      <span style={{ fontSize: 12, color: 'var(--fg-2)' }}>Name</span>
                      <input
                        className="input"
                        value={name}
                        onChange={(e) => setName(e.target.value)}
                        placeholder="e.g. ci-deploy"
                        autoFocus
                      />
                    </label>
                    <p className="dim" style={{ margin: 0, fontSize: 11.5 }}>
                      Token will inherit the <code className="mono">admin</code> scope and expire in 90 days. Use the API to request narrower scopes if needed.
                    </p>
                  </>
                )}
              </div>
              <div className="modal-foot">
                {issuedSecret ? (
                  <button
                    type="button"
                    className="btn primary"
                    onClick={() => {
                      setIssuedSecret(null)
                      setDialogOpen(false)
                    }}
                  >
                    Done
                  </button>
                ) : (
                  <>
                    <Dialog.Close className="btn ghost">Cancel</Dialog.Close>
                    <button type="button" className="btn primary" onClick={issuePat} disabled={!name.trim() || createPat.isPending}>
                      {createPat.isPending ? <BrandMark spinning size={13} /> : null} Issue token
                    </button>
                  </>
                )}
              </div>
            </Dialog.Content>
          </Dialog.Portal>
        </Dialog.Root>
      </div>
      <div style={{ borderTop: '1px solid var(--divider)' }}>
        {tokens.isLoading ? (
          <div className="dim center" style={{ padding: 30 }}>
            Loading…
          </div>
        ) : !tokens.data || tokens.data.length === 0 ? (
          <EmptyState title="No personal access tokens" desc="Use a PAT for CLI tooling and CI without sharing your password." />
        ) : (
          <table className="tbl">
            <thead>
              <tr>
                <th>Name</th>
                <th>Prefix</th>
                <th>Scopes</th>
                <th>Last used</th>
                <th>Expires</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {tokens.data.map((t) => (
                <tr key={t.token_id}>
                  <td>{t.name}</td>
                  <td className="mono dim">{t.token_prefix}…</td>
                  <td className="dim mono" style={{ fontSize: 11.5 }}>
                    {t.scopes.join(', ')}
                  </td>
                  <td className="dim">{t.last_used_at ? formatRelative(t.last_used_at) : 'never'}</td>
                  <td className="dim">{t.expires_at ? formatRelative(t.expires_at) : '—'}</td>
                  <td style={{ textAlign: 'right' }}>
                    <button
                      type="button"
                      className="btn icon sm danger-hover"
                      onClick={() => revoke(t.token_id, t.name)}
                      title="Revoke"
                      aria-label={`Revoke ${t.name}`}
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
  )
}

function InfoRow({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="row" style={{ fontSize: 12.5 }}>
      <span className="dim" style={{ width: 110 }}>{label}</span>
      <span style={{ color: 'var(--fg)' }}>{value}</span>
    </div>
  )
}
