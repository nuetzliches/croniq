import { useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { Lock, Shield, ShieldCheck, ArrowRight, ExternalLink, Bell } from 'lucide-react'
import clsx from 'clsx'
import { useAuthStore } from './store'
import { classifyLoginFailure } from './login-failure'
import { apiFetch, apiPost } from '@/api/client'
import { isMac } from '@/lib/utils'
import {
  isMfaRequired,
  isEnrollmentRequired,
  type AuthConfigResponse,
  type HealthResponse,
  type LoginResponse,
  type TokenResponse,
  type TotpSetupResponse,
  type VersionResponse,
} from '@/api/types'
import { BrandMark, EnvBadge, CopyBtn } from '@/components/primitives'
import { OtpInput } from '@/components/OtpInput'
import { TotpQr } from '@/components/TotpQr'
import { useVersion } from '@/api/hooks'

type Method = 'password' | 'sso'

const DOCS_URL = 'https://nuetzliches.github.io/croniq/'

// Hero verb rotation — each verb stays ~3.5s before fading to the next.
const VERBS = ['Recover', 'Replay', 'Diagnose', 'Audit', 'Scale']

// Demo console — types a CLI command at the top, then streams the
// output that running it would produce. Each entry is one full demo:
// the command (`cmd` + optional `arg`) plus the lines that follow.
interface DemoLine {
  lvl: 'info' | 'warn' | 'error' | 'ok' | 'debug'
  text: string
}
interface Demo {
  cmd: string
  arg?: string
  output: DemoLine[]
}
// Each entry is a real subcommand of the `croniq` CLI (see
// crates/croniq-cli/src/main.rs). The output lines are illustrative but
// shaped after the actual command's responsibility — nothing here
// implies a feature the binary doesn't ship.
const DEMOS: Demo[] = [
  {
    cmd: 'croniq quickstart',
    output: [
      { lvl: 'info', text: 'wrote ./Croniqfile with a sample heartbeat job' },
      { lvl: 'info', text: 'created ./.data/croniq.db · admin user provisioned' },
      { lvl: 'ok', text: 'next: croniq-server --config Croniqfile --data-dir ./.data' },
    ],
  },
  {
    cmd: 'croniq validate',
    arg: 'Croniqfile',
    output: [
      { lvl: 'info', text: 'parsed Croniqfile · 3 jobs · 1 calendar · 1 runner pool' },
      { lvl: 'info', text: 'no scheduling collisions · all rules reachable' },
      { lvl: 'ok', text: 'Croniqfile is valid' },
    ],
  },
  {
    cmd: 'croniq trigger',
    arg: 'demo:heartbeat',
    output: [
      { lvl: 'info', text: 'POST /v1/trigger → execution ex_5a8c2244 queued' },
      { lvl: 'info', text: 'claimed by runner shell-runner-7b31d0ee' },
      { lvl: 'ok', text: 'completed in 1.4s · exit 0' },
    ],
  },
  {
    cmd: 'croniq status',
    output: [
      { lvl: 'info', text: 'http://localhost:4000 · 2 runners online · 0 stale' },
      { lvl: 'info', text: 'queue depth 0 · 12 jobs registered' },
      { lvl: 'ok', text: 'scheduler healthy' },
    ],
  },
  {
    cmd: 'croniq list-runners',
    output: [
      { lvl: 'info', text: 'shell-runner-7b31d0ee · online · last poll 2s ago' },
      { lvl: 'info', text: 'shell-runner-954fa504 · online · capabilities: shell' },
      { lvl: 'ok', text: '2/2 runners online · 0 stale · 0 dead' },
    ],
  },
  {
    cmd: 'croniq convert',
    arg: "'0 9 * * 1-5'",
    output: [
      { lvl: 'info', text: 'parsed as 5-field standard cron' },
      { lvl: 'ok', text: 'DSL: every weekday at 09:00' },
    ],
  },
  {
    cmd: 'croniq fmt',
    arg: '-w Croniqfile',
    output: [
      { lvl: 'info', text: 'normalised whitespace · sorted top-level blocks' },
      { lvl: 'info', text: 'wrote Croniqfile · 12 lines reflowed' },
      { lvl: 'ok', text: 'formatting clean' },
    ],
  },
  {
    cmd: 'croniq diff',
    arg: 'Croniqfile.old Croniqfile',
    output: [
      { lvl: 'info', text: 'compared 2 files · 1 add · 3 mod · 0 del' },
      { lvl: 'info', text: '+ job "payroll" · timeout 5m → 10m · calendar attached' },
      { lvl: 'ok', text: 'diff complete' },
    ],
  },
  {
    cmd: 'croniq migrate',
    arg: '/etc/crontab -o Croniqfile',
    output: [
      { lvl: 'info', text: 'parsed 7 entries from /etc/crontab' },
      { lvl: 'warn', text: 'skipped 1 line (unparseable @reboot directive)' },
      { lvl: 'ok', text: 'wrote Croniqfile · review before deploying' },
    ],
  },
  {
    cmd: 'croniq dead-letters',
    arg: '--data-dir ./.data --job payroll',
    output: [
      { lvl: 'info', text: 'found 2 dead letters for payroll in croniq.db' },
      { lvl: 'info', text: 'ex_98a4… · attempt 3/3 · exit 137 · OOMKilled · 26d ago' },
      { lvl: 'ok', text: 'use `dead-letters-inspect <id>` for stdout/stderr' },
    ],
  },
]

const VERB_INTERVAL_MS = 3400
const TYPE_CHAR_MS = 38
const PAUSE_AFTER_TYPING_MS = 380
const OUTPUT_LINE_MS = 320
const HOLD_AFTER_OUTPUT_MS = 5000
const CLEAR_FADE_MS = 220

export function LoginPage() {
  const [method, setMethod] = useState<Method>('password')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [totpRequired, setTotpRequired] = useState(false)
  const [mfaCode, setMfaCode] = useState('')
  const [useRecovery, setUseRecovery] = useState(false)
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const [shake, setShake] = useState(false)
  const [authCfg, setAuthCfg] = useState<AuthConfigResponse | null>(null)
  const [health, setHealth] = useState<HealthResponse | null>(null)
  // Forced TOTP enrolment (enforced 2FA, account not yet enrolled).
  const [enrollToken, setEnrollToken] = useState<string | null>(null)
  const [enrollData, setEnrollData] = useState<TotpSetupResponse | null>(null)
  const [enrollCode, setEnrollCode] = useState('')
  const [enrollAck, setEnrollAck] = useState(false)

  const login = useAuthStore((s) => s.login)
  const navigate = useNavigate()

  useEffect(() => {
    // Combined sign-in-method probe — surfaces both `oidc.enabled` and
    // `password.enabled` so the LoginPage can hide whichever flow the
    // operator has turned off (issue #138).
    apiFetch<AuthConfigResponse>('/v1/auth/config')
      .then(setAuthCfg)
      .catch(() =>
        setAuthCfg({
          oidc: { enabled: false, provider_name: null, login_url: null },
          password: { enabled: true },
          totp: { required: false },
        }),
      )
    apiFetch<HealthResponse>('/health').then(setHealth, () => setHealth(null))
  }, [])

  const oidc = authCfg?.oidc ?? null
  const passwordEnabled = authCfg?.password.enabled ?? true
  // When password login is disabled by the operator, force the SSO view —
  // regardless of whatever `method` the user clicked. The tab strip is
  // hidden in this state so there's nothing to switch back to.
  const effectiveMethod: Method = passwordEnabled ? method : 'sso'
  const showTabStrip = passwordEnabled && !!oidc?.enabled
  const bothDisabled =
    authCfg !== null && !passwordEnabled && !oidc?.enabled
  // Server-enforced 2FA: show the code field from the start and submit it
  // inline (single request). Otherwise the field is revealed only after the
  // credential probe reports the account has 2FA.
  const totpEnforced = authCfg?.totp.required ?? false
  const showTotpField = totpEnforced || totpRequired

  function flashError(msg: string) {
    setError(msg)
    setShake(true)
    window.setTimeout(() => setShake(false), 400)
  }

  // Classification lives in `login-failure.ts` so it is unit-testable — the
  // wording decides where an operator starts looking, and a 5xx must not read
  // as "the backend is down" (issue #410).
  function reportFailure(err: unknown, hadCode = false) {
    flashError(classifyLoginFailure(err, hadCode).message)
  }

  // One submit handler, one request. We POST username + password (+ the code
  // if the field is filled) to /v1/auth/login. The server verifies the second
  // factor inline and returns tokens directly. If the account has 2FA and we
  // didn't send a code, the server answers `requires_totp` and we reveal the
  // field for a follow-up submit. When 2FA is enforced server-wide the field
  // is shown from the start, so enforced logins are a single round trip.
  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    setError('')
    setLoading(true)
    try {
      const body: Record<string, string> = { username, password }
      const code = mfaCode.trim()
      if (code) {
        if (useRecovery) body.recovery_code = code
        else body.code = code
      }
      const res = await apiPost<LoginResponse>('/v1/auth/login', body)
      if (isMfaRequired(res)) {
        // Account has 2FA but no code was supplied — reveal the field.
        setTotpRequired(true)
        flashError(
          useRecovery
            ? 'Enter one of your recovery codes.'
            : 'Enter the 6-digit code from your authenticator app.',
        )
        return
      }
      if (isEnrollmentRequired(res)) {
        // Enforced 2FA, account not yet enrolled → start inline TOTP setup
        // instead of locking the user out.
        await beginEnrollment(res.enroll_token)
        return
      }
      login(res.access_token, res.refresh_token)
      navigate('/')
    } catch (err) {
      // A code was sent, so a 401 is the password or the code — the classifier
      // names both in that case.
      reportFailure(err, mfaCode.trim().length > 0)
    } finally {
      setLoading(false)
    }
  }

  // Fetch the enrolment material (QR + secret + recovery codes) for the
  // short-lived enroll token, then switch the form to the enrol view.
  async function beginEnrollment(token: string) {
    setEnrollToken(token)
    setEnrollCode('')
    setEnrollAck(false)
    setError('')
    try {
      const data = await apiPost<TotpSetupResponse>(
        '/v1/auth/login/enroll/totp/begin',
        { enroll_token: token },
      )
      setEnrollData(data)
    } catch {
      flashError('Could not start two-factor setup. Please sign in again.')
      setEnrollToken(null)
    }
  }

  async function confirmEnrollment(e: React.FormEvent) {
    e.preventDefault()
    if (!enrollToken) return
    setError('')
    setLoading(true)
    try {
      const tokens = await apiPost<TokenResponse>(
        '/v1/auth/login/enroll/totp/confirm',
        { enroll_token: enrollToken, code: enrollCode.trim() },
      )
      login(tokens.access_token, tokens.refresh_token)
      navigate('/')
    } catch (err) {
      const msg = err instanceof Error ? err.message : ''
      if (/^401[: ]/.test(msg)) {
        flashError("That code didn't match. Enter the current 6-digit code.")
      } else {
        flashError('Could not finish two-factor setup. Please try again.')
      }
    } finally {
      setLoading(false)
    }
  }

  function cancelEnrollment() {
    setEnrollToken(null)
    setEnrollData(null)
    setEnrollCode('')
    setEnrollAck(false)
    setError('')
  }

  // Editing username/password clears a *revealed* 2FA prompt (enforced ones
  // stay visible); the next submit re-probes from scratch.
  function onCredentialChange(setter: (v: string) => void) {
    return (e: React.ChangeEvent<HTMLInputElement>) => {
      setter(e.target.value)
      if (totpRequired) {
        setTotpRequired(false)
        setMfaCode('')
        setUseRecovery(false)
        setError('')
      }
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
      <div className="login-stage-bg" aria-hidden>
        <div className="login-spot login-spot-a" />
        <div className="login-spot login-spot-b" />
      </div>
      <div className="login-formwrap">
        <LoginCompactStrip health={health} />
        <div className={clsx('login-form', shake && 'shake')}>
          <div className="login-mark" aria-hidden>
            <BrandMark size={22} />
          </div>
            <>
              <h1 className="login-form-title">Welcome back</h1>
              <p className="login-form-sub">
                Sign in to <span className="mono" style={{ color: 'var(--fg-1)' }}>{window.location.host}</span>
              </p>

              {/* Only render the tab strip when both methods are actually
                  available. Showing a single, always-selected "Password"
                  tab is visual noise; showing SSO disabled exposes a
                  feature the user has no path to enable. */}
              {showTabStrip ? (
                <div
                  className="login-method-tabs"
                  role="tablist"
                  aria-label="Sign-in method"
                  style={{ gridTemplateColumns: 'repeat(2, 1fr)' }}
                >
                  <LoginTab id="password" current={method} setMethod={setMethod} icon={Lock} label="Password" />
                  <LoginTab id="sso" current={method} setMethod={setMethod} icon={Shield} label="SSO" />
                </div>
              ) : null}

              {bothDisabled ? (
                <div className="col gap-14">
                  <div className="login-sso-card">
                    <span className="login-sso-icon">
                      <Lock size={18} />
                    </span>
                    <div className="col" style={{ gap: 0, flex: 1, minWidth: 0 }}>
                      <span style={{ fontSize: 13.5, fontWeight: 500, color: 'var(--fg)' }}>
                        No sign-in method configured
                      </span>
                      <span className="dim" style={{ fontSize: 11.5 }}>
                        Password login is disabled and SSO is not set up.
                      </span>
                    </div>
                  </div>
                  <p className="dim" style={{ fontSize: 12.5, textAlign: 'center', margin: 0 }}>
                    Contact your Croniq administrator to enable a UI sign-in method.
                  </p>
                </div>
              ) : null}

              {!bothDisabled && effectiveMethod === 'password' ? (
                enrollData ? (
                  <LoginEnrollView
                    data={enrollData}
                    code={enrollCode}
                    onCode={setEnrollCode}
                    ack={enrollAck}
                    onAck={setEnrollAck}
                    loading={loading}
                    error={error}
                    onConfirm={confirmEnrollment}
                    onCancel={cancelEnrollment}
                  />
                ) : (
                <form className="col gap-14" onSubmit={handleSubmit}>
                  <LoginField label="Username">
                    <input
                      className="input"
                      value={username}
                      onChange={onCredentialChange(setUsername)}
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
                      onChange={onCredentialChange(setPassword)}
                      autoComplete="current-password"
                      placeholder="•••••••••"
                      required
                    />
                  </LoginField>
                  {showTotpField ? (
                    <LoginField
                      label={useRecovery ? 'Recovery code' : 'Two-factor code'}
                      hint={
                        totpEnforced
                          ? 'Two-factor is required to sign in.'
                          : 'Required because two-factor is enabled on this account.'
                      }
                    >
                      {useRecovery ? (
                        <input
                          className="input mono"
                          inputMode="text"
                          maxLength={8}
                          value={mfaCode}
                          onChange={(e) => setMfaCode(e.target.value)}
                          placeholder="xxxxxxxx"
                          autoComplete="one-time-code"
                          autoFocus
                          style={{ textAlign: 'center', letterSpacing: '0.4em', fontSize: 16, height: 40 }}
                        />
                      ) : (
                        <OtpInput
                          value={mfaCode}
                          onChange={setMfaCode}
                          length={6}
                          autoFocus={totpRequired}
                        />
                      )}
                      <button
                        type="button"
                        onClick={() => {
                          setUseRecovery((v) => !v)
                          setMfaCode('')
                          setError('')
                        }}
                        style={{ background: 'transparent', border: 0, color: 'var(--fg-3)', cursor: 'pointer', padding: 0, font: 'inherit', fontSize: 11.5, textAlign: 'left' }}
                      >
                        {useRecovery ? 'Use authenticator code' : 'Use a recovery code instead'}
                      </button>
                    </LoginField>
                  ) : null}
                  {error ? <ErrorBanner msg={error} /> : null}
                  <SubmitButton loading={loading}>
                    {loading
                      ? showTotpField
                        ? 'Verifying…'
                        : 'Signing in…'
                      : showTotpField
                        ? 'Verify & sign in'
                        : 'Sign in'}
                    {loading ? null : <ArrowRight size={14} />}
                  </SubmitButton>
                </form>
                )
              ) : null}

              {!bothDisabled && effectiveMethod === 'sso' ? (
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

              {/* Password-reset recovery is only meaningful when password
                  login is on — hide the section entirely otherwise. */}
              {passwordEnabled ? (
                <>
                  <div className="login-divider">
                    <span>Lost access?</span>
                  </div>
                  <div className="row gap-10" style={{ justifyContent: 'center', flexWrap: 'wrap' }}>
                    <button type="button" className="login-recovery" onClick={handleForgotPassword}>
                      <Bell size={12} /> Email a recovery link
                    </button>
                  </div>
                </>
              ) : null}
            </>
        </div>

        <div className="login-formfoot">
          <span>{isMac ? '⌘' : 'Ctrl'}<span style={{ letterSpacing: 1 }}>↵</span> sign in</span>
          <span>·</span>
          <span>Tab to switch fields</span>
        </div>
      </div>
    </div>
  )
}

function LoginStage({ health }: { health: HealthResponse | null }) {
  const [verbIdx, setVerbIdx] = useState(0)
  const { data: version } = useVersion()

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
      <div className="login-brand">
        <span className="login-mark" aria-hidden>
          <BrandMark size={22} />
        </span>
        <span className="login-name">Croniq</span>
        {version ? <VersionChip version={version} /> : null}
        {version ? <EnvBadge env={version.env} /> : null}
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

      <LoginDemoConsole />

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

type DemoPhase = 'typing' | 'output' | 'hold' | 'clearing'

function LoginDemoConsole() {
  const [demoIdx, setDemoIdx] = useState(0)
  const [typed, setTyped] = useState('')
  const [shownLines, setShownLines] = useState(0)
  const [phase, setPhase] = useState<DemoPhase>('typing')
  const [paused, setPaused] = useState(false)
  // Track elapsed-during-hold so hovering off mid-countdown picks up
  // exactly where it left off instead of restarting the full 5 s.
  const holdElapsedRef = useRef(0)

  const demo = DEMOS[demoIdx]
  const fullCmd = demo.arg ? `${demo.cmd} ${demo.arg}` : demo.cmd

  // Single ticking state-machine. The HOLD phase is the only one that
  // respects `paused` — typing and output ticks are too short for a
  // hover pause to be useful, and clearing is intentionally fast.
  useEffect(() => {
    if (phase === 'typing') {
      if (typed.length < fullCmd.length) {
        const t = window.setTimeout(
          () => setTyped(fullCmd.slice(0, typed.length + 1)),
          TYPE_CHAR_MS,
        )
        return () => window.clearTimeout(t)
      }
      const t = window.setTimeout(() => setPhase('output'), PAUSE_AFTER_TYPING_MS)
      return () => window.clearTimeout(t)
    }
    if (phase === 'output') {
      if (shownLines < demo.output.length) {
        const t = window.setTimeout(() => setShownLines((n) => n + 1), OUTPUT_LINE_MS)
        return () => window.clearTimeout(t)
      }
      // Entering hold — reset the elapsed counter so the next time
      // pause/resume runs it starts from zero.
      holdElapsedRef.current = 0
      const t = window.setTimeout(() => setPhase('hold'), 0)
      return () => window.clearTimeout(t)
    }
    if (phase === 'hold') {
      if (paused) return undefined
      const remaining = Math.max(0, HOLD_AFTER_OUTPUT_MS - holdElapsedRef.current)
      const startedAt = Date.now()
      const t = window.setTimeout(() => setPhase('clearing'), remaining)
      return () => {
        // Capture elapsed time so a follow-up resume restarts from the
        // right offset. Only count it once per pause cycle by clamping
        // to HOLD_AFTER_OUTPUT_MS.
        holdElapsedRef.current = Math.min(
          HOLD_AFTER_OUTPUT_MS,
          holdElapsedRef.current + (Date.now() - startedAt),
        )
        window.clearTimeout(t)
      }
    }
    // clearing → wait one fade beat, then reset to the next demo. The
    // timeout indirection keeps the state mutation out of the effect
    // body so React's set-state-in-effect rule stays happy.
    const t = window.setTimeout(() => {
      setTyped('')
      setShownLines(0)
      setDemoIdx((i) => (i + 1) % DEMOS.length)
      setPhase('typing')
    }, CLEAR_FADE_MS)
    return () => window.clearTimeout(t)
  }, [phase, typed, shownLines, fullCmd, demo.output.length, paused])

  const stillTyping = phase === 'typing' && typed.length < fullCmd.length
  const fading = phase === 'clearing'
  const headerLabel =
    phase === 'typing'
      ? 'typing'
      : phase === 'output'
        ? 'running'
        : phase === 'hold'
          ? paused ? 'paused' : 'idle'
          : null

  return (
    <div
      className="login-console"
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
    >
      <div className="login-console-head">
        <div className="login-traffic">
          <span style={{ background: 'oklch(0.65 0.18 25)' }} />
          <span style={{ background: 'oklch(0.78 0.16 75)' }} />
          <span style={{ background: 'oklch(0.70 0.16 145)' }} />
        </div>
        <span className="mono dim" style={{ fontSize: 11 }}>
          ~ croniq · live demo
        </span>
        {headerLabel ? (
          <span
            className="row gap-6"
            style={{ marginLeft: 'auto', fontSize: 10.5, color: 'var(--fg-3)' }}
          >
            <span className="live-dot" />
            {headerLabel}
          </span>
        ) : null}
      </div>
      <div className={`login-console-body${fading ? ' login-console-fading' : ''}`}>
        <div className="login-console-line" style={{ color: 'var(--fg)' }}>
          <span style={{ color: 'var(--accent-3)', marginRight: 8 }}>$</span>
          <span className="mono" style={{ whiteSpace: 'pre' }}>
            {typed}
          </span>
          {stillTyping || phase === 'typing' ? (
            <span className="login-blink" aria-hidden>
              ▌
            </span>
          ) : null}
        </div>
        {Array.from({ length: shownLines }).map((_, i) => {
          const line = demo.output[i]
          return (
            <div
              key={`${demoIdx}-${i}`}
              className="login-console-line login-console-output"
            >
              <span className={`lvl-${line.lvl}`}>{line.lvl}</span>
              <span style={{ color: 'var(--fg-1)' }}>{line.text}</span>
            </div>
          )
        })}
        {phase === 'output' || phase === 'hold' ? (
          <div className="login-console-line login-console-cursor" style={{ marginTop: 4 }}>
            <span style={{ color: 'var(--accent-3)', marginRight: 8 }}>$</span>
            <span className="login-blink" aria-hidden>
              ▌
            </span>
          </div>
        ) : null}
      </div>
      {phase === 'hold' ? (
        <div
          key={demoIdx}
          className="login-console-countdown"
          style={{
            animationDuration: `${HOLD_AFTER_OUTPUT_MS}ms`,
            animationPlayState: paused ? 'paused' : 'running',
          }}
          aria-hidden
        />
      ) : null}
    </div>
  )
}

function VersionChip({ version }: { version: VersionResponse }) {
  return (
    <span
      className="tag mono"
      title={`Build ${version.git_sha} · ${version.build_time}`}
      style={{ height: 20, fontSize: 11 }}
    >
      v{version.version}
    </span>
  )
}

/** Narrow-viewport companion to <LoginStage />. Renders brand + version/
 *  env chips and a slim three-up stats row above the form so the
 *  marketing context (who you are, where you are, what's running) stays
 *  on screen even when the full stage panel is hidden. */
function LoginCompactStrip({ health }: { health: HealthResponse | null }) {
  const { data: version } = useVersion()
  const runnersOnline = health?.runners_online ?? 0
  const runnersStale = health?.runners_stale ?? 0
  const runnersDead = health?.runners_dead ?? 0
  const runnersTotal = runnersOnline + runnersStale + runnersDead
  const queued = health?.queued ?? 0
  const status = health?.status ?? '—'
  const runnersTone: 'up' | 'warn' = runnersStale > 0 || runnersDead > 0 ? 'warn' : 'up'
  const statusTone: 'up' | 'warn' = status === 'ok' ? 'up' : 'warn'

  return (
    <div className="login-compact-strip">
      <div className="login-compact-brand">
        <span className="login-mark" aria-hidden>
          <BrandMark size={20} />
        </span>
        <span className="login-name">Croniq</span>
        {version ? <VersionChip version={version} /> : null}
        {version ? <EnvBadge env={version.env} /> : null}
        <span
          className="row gap-6"
          style={{ marginLeft: 'auto', color: 'var(--fg-3)', fontSize: 11 }}
        >
          <span className="live-dot" />
          <span className="mono">{window.location.host}</span>
        </span>
      </div>
      <div className="login-stats">
        <div className="login-stat">
          <div className="login-stat-label">queue</div>
          <div className="login-stat-value">{queued}</div>
          <div className="login-stat-sub login-stat-up">awaiting fire</div>
        </div>
        <div className="login-stat">
          <div className="login-stat-label">runners</div>
          <div className="login-stat-value">
            {runnersOnline} / {runnersTotal || '—'}
          </div>
          <div className={`login-stat-sub login-stat-${runnersTone}`}>
            {runnersStale > 0 ? `${runnersStale} stale` : 'all healthy'}
          </div>
        </div>
        <div className="login-stat">
          <div className="login-stat-label">status</div>
          <div className="login-stat-value">{status}</div>
          <div className={`login-stat-sub login-stat-${statusTone}`}>
            {status === 'ok' ? 'operational' : 'check /health'}
          </div>
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
      {loading ? <BrandMark spinning size={14} /> : null}
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

/** Inline "set up 2FA to continue" view shown when enforced 2FA is on and the
 *  account has no TOTP yet. Mirrors Settings → Two-factor setup, but completes
 *  the login on confirm. */
function LoginEnrollView({
  data,
  code,
  onCode,
  ack,
  onAck,
  loading,
  error,
  onConfirm,
  onCancel,
}: {
  data: TotpSetupResponse
  code: string
  onCode: (v: string) => void
  ack: boolean
  onAck: (v: boolean) => void
  loading: boolean
  error: string
  onConfirm: (e: React.FormEvent) => void
  onCancel: () => void
}) {
  return (
    <form className="col gap-14" onSubmit={onConfirm}>
      <div className="banner warn" role="status" style={{ fontSize: 12.5 }}>
        <span className="grow">
          Two-factor is required here. Set it up now to finish signing in.
        </span>
      </div>

      <div className="row" style={{ gap: 14, alignItems: 'flex-start', flexWrap: 'wrap' }}>
        <TotpQr value={data.otpauth_url} size={150} />
        <div className="col" style={{ gap: 8, flex: '1 1 200px', minWidth: 0 }}>
          <p style={{ margin: 0, fontSize: 12.5 }}>
            Scan with your authenticator app, or enter this secret manually:
          </p>
          <div className="row" style={{ gap: 6 }}>
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
              {data.secret}
            </code>
            <CopyBtn value={data.secret} />
          </div>
        </div>
      </div>

      <div
        className="banner warn"
        role="status"
        style={{ flexDirection: 'column', alignItems: 'flex-start', gap: 6 }}
      >
        <span className="grow">
          <strong>Save these recovery codes.</strong> They're shown once and let you sign in if you
          lose your authenticator.
        </span>
        <ul className="mono" style={{ margin: 0, paddingLeft: 18, columns: 2, fontSize: 11.5 }}>
          {data.recovery_codes.map((c, i) => (
            <li key={i}>{c}</li>
          ))}
        </ul>
        <div className="row" style={{ gap: 8, marginTop: 4, alignItems: 'center' }}>
          <CopyBtn value={data.recovery_codes.join('\n')} label="Copy all" />
          <label className="row" style={{ gap: 6, fontSize: 12 }}>
            <input type="checkbox" checked={ack} onChange={(e) => onAck(e.target.checked)} />I saved
            them
          </label>
        </div>
      </div>

      <LoginField label="Two-factor code" hint="Enter the 6-digit code to finish.">
        <OtpInput value={code} onChange={onCode} length={6} autoFocus />
      </LoginField>

      {error ? <ErrorBanner msg={error} /> : null}

      <SubmitButton loading={loading} disabled={!ack || code.length !== 6}>
        {loading ? 'Enabling…' : 'Enable & sign in'}
        {loading ? null : <ShieldCheck size={14} />}
      </SubmitButton>
      <button
        type="button"
        onClick={onCancel}
        style={{
          background: 'transparent',
          border: 0,
          color: 'var(--fg-3)',
          cursor: 'pointer',
          font: 'inherit',
          fontSize: 11.5,
        }}
      >
        Back to sign-in
      </button>
    </form>
  )
}

