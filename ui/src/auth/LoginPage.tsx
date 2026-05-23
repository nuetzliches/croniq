import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { Settings2 } from 'lucide-react'
import { useAuthStore } from './store'
import { apiFetch, apiPost } from '@/api/client'
import {
  isMfaRequired,
  type LoginResponse,
  type OidcConfigResponse,
  type TokenResponse,
} from '@/api/types'

type Step = 'credentials' | 'mfa'

export function LoginPage() {
  const [step, setStep] = useState<Step>('credentials')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  // MFA step state — preserved across the two-step flow.
  const [mfaToken, setMfaToken] = useState('')
  const [mfaCode, setMfaCode] = useState('')
  const [useRecovery, setUseRecovery] = useState(false)
  // OIDC config probe — hides the SSO button when the server has no
  // CRONIQ_OIDC_* configured.
  const [oidc, setOidc] = useState<OidcConfigResponse | null>(null)

  const login = useAuthStore((s) => s.login)
  const navigate = useNavigate()

  useEffect(() => {
    apiFetch<OidcConfigResponse>('/v1/auth/oidc/config')
      .then(setOidc)
      .catch(() => setOidc({ enabled: false, provider_name: null, login_url: null }))
  }, [])

  function reportFailure(err: unknown) {
    const msg = err instanceof Error ? err.message : ''
    const unreachable = err instanceof TypeError || /^5\d\d[: ]/.test(msg)
    if (unreachable) {
      setError('Cannot reach server. Check that the Croniq backend is running.')
    } else if (/^401[: ]/.test(msg)) {
      setError('Invalid credentials.')
    } else if (/^403[: ]/.test(msg)) {
      setError('Account is locked or inactive. Contact an admin.')
    } else {
      setError('Login failed. Check your credentials.')
    }
  }

  async function handleCredentialsSubmit(e: React.FormEvent) {
    e.preventDefault()
    setError('')
    setLoading(true)
    try {
      const res = await apiPost<LoginResponse>('/v1/auth/login', { username, password })
      if (isMfaRequired(res)) {
        setMfaToken(res.mfa_token)
        setStep('mfa')
      } else {
        login(res.access_token, res.refresh_token)
        navigate('/')
      }
    } catch (err) {
      reportFailure(err)
    } finally {
      setLoading(false)
    }
  }

  async function handleMfaSubmit(e: React.FormEvent) {
    e.preventDefault()
    setError('')
    setLoading(true)
    try {
      const body: Record<string, string> = { mfa_token: mfaToken }
      if (useRecovery) body.recovery_code = mfaCode.trim()
      else body.code = mfaCode.trim()
      const res = await apiPost<TokenResponse>('/v1/auth/login/totp', body)
      login(res.access_token, res.refresh_token)
      navigate('/')
    } catch (err) {
      reportFailure(err)
    } finally {
      setLoading(false)
    }
  }

  return (
    <div
      style={{
        minHeight: '100vh',
        display: 'grid',
        placeItems: 'center',
        background: 'var(--bg)',
        padding: 20,
      }}
    >
      <div
        className="panel"
        style={{
          width: '100%',
          maxWidth: 380,
          padding: 28,
          boxShadow: 'var(--shadow-lg)',
        }}
      >
        <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 6, marginBottom: 18 }}>
          <span
            className="center"
            style={{
              width: 38,
              height: 38,
              borderRadius: 9,
              background: 'var(--accent)',
              color: 'white',
              marginBottom: 6,
            }}
          >
            <Settings2 size={20} />
          </span>
          <h1
            style={{
              margin: 0,
              fontSize: 19,
              fontWeight: 600,
              letterSpacing: '-0.015em',
              color: 'var(--fg)',
            }}
          >
            Sign in to Croniq
          </h1>
          <p className="dim" style={{ fontSize: 12.5, margin: 0 }}>
            Schedule, run and observe cron jobs across your fleet.
          </p>
        </div>

        {step === 'credentials' ? (
          <CredentialsForm
            username={username}
            password={password}
            error={error}
            loading={loading}
            onUsername={setUsername}
            onPassword={setPassword}
            onSubmit={handleCredentialsSubmit}
            oidc={oidc}
          />
        ) : (
          <MfaForm
            code={mfaCode}
            error={error}
            loading={loading}
            useRecovery={useRecovery}
            onCode={setMfaCode}
            onSubmit={handleMfaSubmit}
            onToggleRecovery={() => {
              setUseRecovery((v) => !v)
              setMfaCode('')
              setError('')
            }}
            onCancel={() => {
              setStep('credentials')
              setMfaToken('')
              setMfaCode('')
              setError('')
              setUseRecovery(false)
            }}
          />
        )}
      </div>
    </div>
  )
}

interface CredentialsFormProps {
  username: string
  password: string
  error: string
  loading: boolean
  oidc: OidcConfigResponse | null
  onUsername: (v: string) => void
  onPassword: (v: string) => void
  onSubmit: (e: React.FormEvent) => void
}

function CredentialsForm({
  username,
  password,
  error,
  loading,
  oidc,
  onUsername,
  onPassword,
  onSubmit,
}: CredentialsFormProps) {
  return (
    <form onSubmit={onSubmit} style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
      <Field label="Username">
        <input
          type="text"
          className="input"
          value={username}
          onChange={(e) => onUsername(e.target.value)}
          autoComplete="username"
          required
          autoFocus
        />
      </Field>
      <Field label="Password">
        <input
          type="password"
          className="input"
          value={password}
          onChange={(e) => onPassword(e.target.value)}
          autoComplete="current-password"
          required
        />
      </Field>
      {error ? (
        <div className="banner error" role="alert" style={{ fontSize: 12.5 }}>
          <span className="grow">{error}</span>
        </div>
      ) : null}
      <button type="submit" disabled={loading} className="btn primary" style={{ height: 36, marginTop: 2 }}>
        {loading ? 'Signing in…' : 'Sign in'}
      </button>

      {oidc?.enabled && oidc.login_url ? (
        <>
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 10,
              margin: '4px 0',
              color: 'var(--fg-mute)',
              fontSize: 11,
            }}
          >
            <span style={{ flex: 1, height: 1, background: 'var(--divider)' }} />
            or continue with
            <span style={{ flex: 1, height: 1, background: 'var(--divider)' }} />
          </div>
          <a
            href={oidc.login_url}
            className="btn"
            style={{ height: 36, textDecoration: 'none' }}
          >
            Sign in with {oidc.provider_name ?? 'SSO'}
          </a>
        </>
      ) : null}
    </form>
  )
}

interface MfaFormProps {
  code: string
  error: string
  loading: boolean
  useRecovery: boolean
  onCode: (v: string) => void
  onSubmit: (e: React.FormEvent) => void
  onToggleRecovery: () => void
  onCancel: () => void
}

function MfaForm({
  code,
  error,
  loading,
  useRecovery,
  onCode,
  onSubmit,
  onToggleRecovery,
  onCancel,
}: MfaFormProps) {
  return (
    <form onSubmit={onSubmit} style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
      <p className="dim" style={{ fontSize: 12.5, margin: 0, textAlign: 'center' }}>
        {useRecovery
          ? 'Enter one of your 8-character recovery codes.'
          : 'Enter the 6-digit code from your authenticator app.'}
      </p>
      <input
        type="text"
        inputMode={useRecovery ? 'text' : 'numeric'}
        pattern={useRecovery ? undefined : '[0-9]{6}'}
        maxLength={useRecovery ? 8 : 6}
        value={code}
        onChange={(e) => onCode(e.target.value)}
        placeholder={useRecovery ? 'xxxxxxxx' : '000000'}
        required
        autoFocus
        className="input mono"
        style={{ textAlign: 'center', letterSpacing: '0.4em', fontSize: 16, height: 40 }}
      />
      {error ? (
        <div className="banner error" role="alert" style={{ fontSize: 12.5 }}>
          <span className="grow">{error}</span>
        </div>
      ) : null}
      <button type="submit" disabled={loading} className="btn primary" style={{ height: 36 }}>
        {loading ? 'Verifying…' : 'Verify'}
      </button>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          fontSize: 11.5,
          color: 'var(--fg-3)',
        }}
      >
        <button
          type="button"
          onClick={onToggleRecovery}
          style={{
            background: 'transparent',
            border: 0,
            color: 'inherit',
            cursor: 'pointer',
            padding: 0,
            font: 'inherit',
          }}
        >
          {useRecovery ? 'Use authenticator code' : 'Use recovery code'}
        </button>
        <button
          type="button"
          onClick={onCancel}
          style={{
            background: 'transparent',
            border: 0,
            color: 'inherit',
            cursor: 'pointer',
            padding: 0,
            font: 'inherit',
          }}
        >
          Cancel
        </button>
      </div>
    </form>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
      <span style={{ fontSize: 12, color: 'var(--fg-2)', fontWeight: 500 }}>{label}</span>
      {children}
    </label>
  )
}
