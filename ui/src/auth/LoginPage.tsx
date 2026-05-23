import { useEffect, useMemo, useState } from 'react'
import { useNavigate } from 'react-router'
import {
  Settings2,
  Lock,
  Key,
  Shield,
  ArrowRight,
  ExternalLink,
  Bell,
} from 'lucide-react'
import clsx from 'clsx'
import { useAuthStore } from './store'
import { apiFetch, apiPost } from '@/api/client'
import {
  isMfaRequired,
  type HealthResponse,
  type LoginResponse,
  type OidcConfigResponse,
  type TokenResponse,
} from '@/api/types'

type Method = 'password' | 'token' | 'sso'
type Step = 'credentials' | 'mfa'

const DOCS_URL = 'https://nuetzliches.github.io/croniq/'

// Hero verb rotation — each verb stays ~3.5s before fading to the next.
const VERBS = ['recover', 'replay', 'diagnose', 'audit', 'scale']

// Tip-of-the-day ticker contents. Pairs of (CLI command, one-line note).
// Picked to surface the most useful flags + the features that newcomers
// usually don't find in the README.
interface Tip {
  cmd: string
  arg?: string
  note: string
}
const TIPS: Tip[] = [
  { cmd: 'croniq job register', arg: '--file croniqfile.hcl', note: 'declarative job + schedule from a single file' },
  { cmd: 'croniq trigger', arg: 'payroll', note: 'manual fire — schedule and next-fire untouched' },
  { cmd: 'croniq dead-letter replay', arg: '--since 1h', note: 'requeue everything that failed in the last hour' },
  { cmd: 'croniq job adopt', arg: 'payroll', note: 'edit a DSL-managed job through the API store' },
  { cmd: 'croniq runner attach', arg: '--capability gpu --tag prod', note: 'label runners so routing can target them' },
  { cmd: 'croniq calendar attach', arg: 'payroll eu-business', note: 'exclude holidays without touching the cron' },
  { cmd: 'croniq stats', arg: '--job payroll --days 30', note: 'p50/p95/p99 + success rate over a window' },
  { cmd: 'croniq totp setup', note: 'enable 2FA for your own account from the CLI' },
  { cmd: 'croniq pat new', arg: '--name ci-deploy --scopes jobs:read', note: 'mint a least-privilege personal access token' },
  { cmd: 'croniq audit', arg: '--target job:payroll', note: 'who touched a job, when, with which diff' },
]

const TIPS_VISIBLE = 5
const TIP_INTERVAL_MS = 3800
const VERB_INTERVAL_MS = 3400

export function LoginPage() {
  const [method, setMethod] = useState<Method>('password')
  const [step, setStep] = useState<Step>('credentials')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [token, setToken] = useState('')
  const [mfaToken, setMfaToken] = useState('')
  const [mfaCode, setMfaCode] = useState('')
  const [useRecovery, setUseRecovery] = useState(false)
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const [shake, setShake] = useState(false)
  const [oidc, setOidc] = useState<OidcConfigResponse | null>(null)
  const [health, setHealth] = useState<HealthResponse | null>(null)

  const login = useAuthStore((s) => s.login)
  const navigate = useNavigate()

  useEffect(() => {
    apiFetch<OidcConfigResponse>('/v1/auth/oidc/config')
      .then(setOidc)
      .catch(() => setOidc({ enabled: false, provider_name: null, login_url: null }))
    apiFetch<HealthResponse>('/health').then(setHealth, () => setHealth(null))
  }, [])

  function flashError(msg: string) {
    setError(msg)
    setShake(true)
    window.setTimeout(() => setShake(false), 400)
  }

  function reportFailure(err: unknown) {
    const msg = err instanceof Error ? err.message : ''
    const unreachable = err instanceof TypeError || /^5\d\d[: ]/.test(msg)
    if (unreachable) flashError('Cannot reach server. Check that the Croniq backend is running.')
    else if (/^401[: ]/.test(msg)) flashError('Invalid credentials.')
    else if (/^403[: ]/.test(msg)) flashError('Account is locked or inactive. Contact an admin.')
    else flashError('Login failed. Check your credentials.')
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

  async function handleForgotPassword(e: React.MouseEvent) {
    e.preventDefault()
    if (!username.trim()) {
      flashError('Enter your username first — the reset link is sent to the email on file.')
      return
    }
    try {
      await apiPost('/v1/auth/password-reset/request', { username: username.trim() })
      flashError('If the account exists, a reset link has been sent.')
    } catch {
      flashError('If the account exists, a reset link has been sent.')
    }
  }

  return (
    <div className="login">
      <LoginStage health={health} />
      <div className="login-formwrap">
        <div className={clsx('login-form', shake && 'shake')}>
          <div className="login-mark" aria-hidden>
            <Settings2 size={18} />
          </div>
          {step === 'credentials' ? (
            <>
              <h1 className="login-form-title">Welcome back</h1>
              <p className="login-form-sub">
                Sign in to <span className="mono" style={{ color: 'var(--fg-1)' }}>{window.location.host}</span>
              </p>

              <div className="login-method-tabs" role="tablist" aria-label="Sign-in method">
                <LoginTab id="password" current={method} setMethod={setMethod} icon={Lock} label="Password" />
                <LoginTab id="token" current={method} setMethod={setMethod} icon={Key} label="API token" />
                <LoginTab id="sso" current={method} setMethod={setMethod} icon={Shield} label="SSO" enabled={oidc?.enabled !== false} />
              </div>

              {method === 'password' ? (
                <form className="col gap-14" onSubmit={handleCredentialsSubmit}>
                  <LoginField label="Username">
                    <input
                      className="input"
                      value={username}
                      onChange={(e) => setUsername(e.target.value)}
                      autoComplete="username"
                      autoFocus
                      required
                    />
                  </LoginField>
                  <LoginField label="Password">
                    <input
                      className="input"
                      type="password"
                      value={password}
                      onChange={(e) => setPassword(e.target.value)}
                      autoComplete="current-password"
                      placeholder="•••••••••"
                      required
                    />
                  </LoginField>
                  {error ? <ErrorBanner msg={error} /> : null}
                  <SubmitButton loading={loading}>
                    {loading ? 'Signing in…' : 'Sign in'}
                    {loading ? null : <ArrowRight size={14} />}
                  </SubmitButton>
                </form>
              ) : null}

              {method === 'token' ? (
                <form className="col gap-14" onSubmit={(e) => e.preventDefault()}>
                  <LoginField
                    label="API token"
                    hint="Personal access tokens are for CLI / CI usage. The browser UI requires password or SSO sign-in."
                  >
                    <textarea
                      className="input mono"
                      rows={3}
                      placeholder="croniq_pat_…"
                      value={token}
                      onChange={(e) => setToken(e.target.value)}
                      style={{ height: 'auto', padding: 10, fontSize: 12.5, resize: 'none' }}
                      readOnly
                    />
                  </LoginField>
                  <SubmitButton loading={false} disabled>
                    <Key size={14} /> Use the CLI with this token
                  </SubmitButton>
                </form>
              ) : null}

              {method === 'sso' ? (
                <div className="col gap-14">
                  {oidc?.enabled && oidc.login_url ? (
                    <>
                      <div className="login-sso-card">
                        <span className="login-sso-icon">
                          <Shield size={18} />
                        </span>
                        <div className="col" style={{ gap: 0, flex: 1, minWidth: 0 }}>
                          <span style={{ fontSize: 13.5, fontWeight: 500, color: 'var(--fg)' }}>
                            {oidc.provider_name ?? 'Identity provider'}
                          </span>
                          <span className="dim mono" style={{ fontSize: 11.5 }}>
                            oidc redirect → {new URL(oidc.login_url, window.location.origin).host}
                          </span>
                        </div>
                        <span className="pill success">
                          <span className="dot" /> active
                        </span>
                      </div>
                      <a href={oidc.login_url} className="btn primary" style={{ height: 42, fontSize: 14, textDecoration: 'none' }}>
                        <ExternalLink size={13} /> Continue with {oidc.provider_name ?? 'SSO'}
                      </a>
                      <p className="dim" style={{ fontSize: 11.5, textAlign: 'center', margin: 0 }}>
                        Redirects to your identity provider.
                      </p>
                    </>
                  ) : (
                    <p className="dim" style={{ fontSize: 13, textAlign: 'center', margin: 0 }}>
                      SSO is not configured on this Croniq instance.
                    </p>
                  )}
                </div>
              ) : null}

              <div className="login-divider">
                <span>Lost access?</span>
              </div>
              <div className="row gap-10" style={{ justifyContent: 'center', flexWrap: 'wrap' }}>
                <button type="button" className="login-recovery" onClick={handleForgotPassword}>
                  <Bell size={12} /> Email a recovery link
                </button>
              </div>
            </>
          ) : (
            <MfaStep
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
                setUseRecovery(false)
                setError('')
              }}
            />
          )}
        </div>

        <div className="login-formfoot">
          <span>⌘<span style={{ letterSpacing: 1 }}>↵</span> sign in</span>
          <span>·</span>
          <span>Tab to switch fields</span>
        </div>
      </div>
    </div>
  )
}

function LoginStage({ health }: { health: HealthResponse | null }) {
  const [verbIdx, setVerbIdx] = useState(0)

  useEffect(() => {
    const t = window.setInterval(
      () => setVerbIdx((i) => (i + 1) % VERBS.length),
      VERB_INTERVAL_MS,
    )
    return () => window.clearInterval(t)
  }, [])

  const stats = useMemo(() => {
    const queued = health?.queued ?? 0
    const runnersOnline = health?.runners_online ?? 0
    const runnersStale = health?.runners_stale ?? 0
    const runnersDead = health?.runners_dead ?? 0
    const runnersTotal = runnersOnline + runnersStale + runnersDead
    return {
      queued,
      runnersOnline,
      runnersStale,
      runnersDead,
      runnersTotal,
      status: health?.status ?? '—',
    }
  }, [health])

  return (
    <div className="login-stage">
      <div className="login-grid" />
      <div className="login-glow" />

      <div className="login-brand">
        <span className="login-mark" aria-hidden>
          <Settings2 size={18} />
        </span>
        <span className="login-name">Croniq</span>
        <span className="row gap-6" style={{ marginLeft: 'auto', color: 'var(--fg-3)', fontSize: 11.5 }}>
          <span className="live-dot" />
          <span>{window.location.host}</span>
        </span>
      </div>

      <div className="login-hero">
        <h1 className="login-tag">
          Schedule. Observe.
          <br />
          <span className="login-tag-accent">
            <span key={verbIdx} className="login-tag-verb" aria-live="polite">
              {VERBS[verbIdx]}.
            </span>
          </span>
        </h1>
        <p className="login-sub">
          Self-hosted cron for fleets that outgrew the crontab. A typed DSL, capability-routed runners, calendars, dead-letter triage and an audit log on every mutation.
        </p>
      </div>

      <div className="login-stats">
        <Stat label="queue depth" value={String(stats.queued)} sub="awaiting fire" tone="up" />
        <Stat
          label="runners"
          value={`${stats.runnersOnline} / ${stats.runnersTotal || '—'}`}
          sub={stats.runnersStale > 0 ? `${stats.runnersStale} stale` : 'all healthy'}
          tone={stats.runnersStale > 0 || stats.runnersDead > 0 ? 'warn' : 'up'}
        />
        <Stat
          label="status"
          value={stats.status}
          sub={stats.status === 'ok' ? 'operational' : 'check /health'}
          tone={stats.status === 'ok' ? 'up' : 'warn'}
        />
      </div>

      <LoginTipTicker />

      <div className="login-foot">
        <span>© Croniq</span>
        <span className="dim">·</span>
        <a
          href={DOCS_URL}
          target="_blank"
          rel="noopener noreferrer"
          className="mono dim"
          style={{ textDecoration: 'none' }}
        >
          {new URL(DOCS_URL).host}
        </a>
        <span className="row gap-6" style={{ marginLeft: 'auto' }}>
          <span className="live-dot" />
          {stats.status === 'ok' ? 'all systems operational' : 'backend degraded'}
        </span>
      </div>
    </div>
  )
}

function Stat({
  label,
  value,
  sub,
  tone,
}: {
  label: string
  value: string
  sub: string
  tone: 'up' | 'warn' | 'down'
}) {
  return (
    <div className="login-stat">
      <div className="login-stat-label">{label}</div>
      <div className="login-stat-value">{value}</div>
      <div className={`login-stat-sub login-stat-${tone}`}>{sub}</div>
    </div>
  )
}

function LoginTipTicker() {
  // The visible window slides through TIPS, one entry per tick. Each tip
  // mounts under a fresh React key so its CSS fade-in animation runs on
  // every rotation — cheaper than tracking timestamps.
  const [offset, setOffset] = useState(0)
  useEffect(() => {
    const t = window.setInterval(() => setOffset((n) => n + 1), TIP_INTERVAL_MS)
    return () => window.clearInterval(t)
  }, [])

  const visible = useMemo(
    () => Array.from({ length: TIPS_VISIBLE }, (_, i) => TIPS[(offset + i) % TIPS.length]),
    [offset],
  )

  return (
    <div className="login-console">
      <div className="login-console-head">
        <div className="login-traffic">
          <span style={{ background: 'oklch(0.65 0.18 25)' }} />
          <span style={{ background: 'oklch(0.78 0.16 75)' }} />
          <span style={{ background: 'oklch(0.70 0.16 145)' }} />
        </div>
        <span className="mono dim" style={{ fontSize: 11 }}>
          ~ croniq --help · daily tips
        </span>
        <span className="row gap-6" style={{ marginLeft: 'auto', fontSize: 10.5, color: 'var(--fg-3)' }}>
          <span className="live-dot" />
          rotating
        </span>
      </div>
      <div className="login-console-body">
        {visible.map((tip, i) => (
          // Key by content so only the freshly-appearing tip re-mounts
          // and runs its fade-in animation. The other tips just shift
          // position in the DOM without flickering.
          <div
            key={tip.cmd}
            className="login-console-tip"
            style={{ opacity: 0.55 + i * 0.11 }}
          >
            <div className="cmd">
              <span className="prompt">$</span>
              <span>{tip.cmd}</span>
              {tip.arg ? <span className="arg"> {tip.arg}</span> : null}
            </div>
            <div className="note"># {tip.note}</div>
          </div>
        ))}
        <div className="login-console-line login-console-cursor" style={{ marginTop: 4 }}>
          <span className="prompt" style={{ color: 'var(--accent-3)', marginRight: 8 }}>$</span>
          <span className="login-blink">▌</span>
        </div>
      </div>
    </div>
  )
}

function LoginTab({
  id,
  current,
  setMethod,
  icon: Icon,
  label,
  enabled = true,
}: {
  id: Method
  current: Method
  setMethod: (m: Method) => void
  icon: typeof Lock
  label: string
  enabled?: boolean
}) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={current === id}
      disabled={!enabled}
      className={clsx('login-method-tab', current === id && 'active')}
      onClick={() => setMethod(id)}
      style={!enabled ? { opacity: 0.4, cursor: 'not-allowed' } : undefined}
    >
      <Icon size={13} /> {label}
    </button>
  )
}

function LoginField({
  label,
  hint,
  children,
}: {
  label: string
  hint?: string
  children: React.ReactNode
}) {
  return (
    <div className="col" style={{ gap: 6 }}>
      <label
        className="dim"
        style={{
          fontSize: 11.5,
          letterSpacing: '0.04em',
          textTransform: 'uppercase',
          fontFamily: 'var(--font-mono-app)',
        }}
      >
        {label}
      </label>
      {children}
      {hint ? (
        <div className="dim" style={{ fontSize: 11.5 }}>
          {hint}
        </div>
      ) : null}
    </div>
  )
}

function SubmitButton({
  loading,
  disabled,
  children,
}: {
  loading: boolean
  disabled?: boolean
  children: React.ReactNode
}) {
  return (
    <button
      type="submit"
      className="btn primary"
      disabled={loading || disabled}
      style={{ height: 42, fontSize: 14, marginTop: 4, opacity: loading || disabled ? 0.7 : 1 }}
    >
      {loading ? <span className="login-spinner" /> : null}
      {children}
    </button>
  )
}

function ErrorBanner({ msg }: { msg: string }) {
  return (
    <div className="banner error" role="alert" style={{ fontSize: 12.5 }}>
      <span className="grow">{msg}</span>
    </div>
  )
}

interface MfaStepProps {
  code: string
  error: string
  loading: boolean
  useRecovery: boolean
  onCode: (v: string) => void
  onSubmit: (e: React.FormEvent) => void
  onToggleRecovery: () => void
  onCancel: () => void
}

function MfaStep({
  code,
  error,
  loading,
  useRecovery,
  onCode,
  onSubmit,
  onToggleRecovery,
  onCancel,
}: MfaStepProps) {
  return (
    <>
      <h1 className="login-form-title">Two-factor required</h1>
      <p className="login-form-sub">
        {useRecovery
          ? 'Enter one of your 8-character recovery codes.'
          : 'Enter the 6-digit code from your authenticator app.'}
      </p>
      <form className="col gap-14" onSubmit={onSubmit}>
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
        {error ? <ErrorBanner msg={error} /> : null}
        <SubmitButton loading={loading}>
          {loading ? 'Verifying…' : 'Verify'}
        </SubmitButton>
        <div
          className="row between"
          style={{ fontSize: 11.5, color: 'var(--fg-3)' }}
        >
          <button
            type="button"
            onClick={onToggleRecovery}
            style={{ background: 'transparent', border: 0, color: 'inherit', cursor: 'pointer', padding: 0, font: 'inherit' }}
          >
            {useRecovery ? 'Use authenticator code' : 'Use recovery code'}
          </button>
          <button
            type="button"
            onClick={onCancel}
            style={{ background: 'transparent', border: 0, color: 'inherit', cursor: 'pointer', padding: 0, font: 'inherit' }}
          >
            Cancel
          </button>
        </div>
      </form>
    </>
  )
}
