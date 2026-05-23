import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
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
      // Wrong code → 401, expired mfa_token → 401 too. Generic message.
      reportFailure(err)
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-muted">
      <div className="w-full max-w-sm bg-card rounded-lg border border-border p-8 shadow-sm">
        <div className="flex justify-center mb-6">
          <img src="/favicon.svg" alt="Croniq" className="h-10 w-10" />
        </div>

        {step === 'credentials' ? (
          <>
            <h1 className="text-xl font-semibold text-center mb-6">Sign in to Croniq</h1>
            <form onSubmit={handleCredentialsSubmit} className="space-y-4">
              <div>
                <label className="block text-sm font-medium mb-1">Username</label>
                <input
                  type="text"
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  className="w-full px-3 py-2 border border-border rounded-md bg-background text-sm focus:outline-none focus:ring-2 focus:ring-primary"
                  required
                  autoFocus
                />
              </div>
              <div>
                <label className="block text-sm font-medium mb-1">Password</label>
                <input
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  className="w-full px-3 py-2 border border-border rounded-md bg-background text-sm focus:outline-none focus:ring-2 focus:ring-primary"
                  required
                />
              </div>
              {error && <p className="text-sm text-destructive">{error}</p>}
              <button
                type="submit"
                disabled={loading}
                className="w-full py-2 px-4 bg-primary text-primary-foreground rounded-md text-sm font-medium hover:opacity-90 disabled:opacity-50"
              >
                {loading ? 'Signing in...' : 'Sign in'}
              </button>
            </form>

            {oidc?.enabled && oidc.login_url && (
              <div className="mt-6 pt-6 border-t border-border">
                <a
                  href={oidc.login_url}
                  className="block w-full text-center py-2 px-4 border border-border rounded-md text-sm font-medium hover:bg-muted"
                >
                  Sign in with {oidc.provider_name ?? 'SSO'}
                </a>
              </div>
            )}
          </>
        ) : (
          <>
            <h1 className="text-xl font-semibold text-center mb-2">Two-factor required</h1>
            <p className="text-sm text-muted-foreground text-center mb-6">
              {useRecovery
                ? 'Enter one of your 8-character recovery codes.'
                : 'Enter the 6-digit code from your authenticator app.'}
            </p>
            <form onSubmit={handleMfaSubmit} className="space-y-4">
              <div>
                <input
                  type="text"
                  inputMode={useRecovery ? 'text' : 'numeric'}
                  pattern={useRecovery ? undefined : '[0-9]{6}'}
                  maxLength={useRecovery ? 8 : 6}
                  value={mfaCode}
                  onChange={(e) => setMfaCode(e.target.value)}
                  className="w-full px-3 py-2 border border-border rounded-md bg-background text-center font-mono text-lg tracking-widest focus:outline-none focus:ring-2 focus:ring-primary"
                  placeholder={useRecovery ? 'xxxxxxxx' : '000000'}
                  required
                  autoFocus
                />
              </div>
              {error && <p className="text-sm text-destructive">{error}</p>}
              <button
                type="submit"
                disabled={loading}
                className="w-full py-2 px-4 bg-primary text-primary-foreground rounded-md text-sm font-medium hover:opacity-90 disabled:opacity-50"
              >
                {loading ? 'Verifying...' : 'Verify'}
              </button>
              <div className="flex justify-between text-xs text-muted-foreground">
                <button
                  type="button"
                  className="underline-offset-2 hover:underline"
                  onClick={() => {
                    setUseRecovery((v) => !v)
                    setMfaCode('')
                    setError('')
                  }}
                >
                  {useRecovery ? 'Use authenticator code' : 'Use recovery code'}
                </button>
                <button
                  type="button"
                  className="underline-offset-2 hover:underline"
                  onClick={() => {
                    setStep('credentials')
                    setMfaToken('')
                    setMfaCode('')
                    setError('')
                    setUseRecovery(false)
                  }}
                >
                  Cancel
                </button>
              </div>
            </form>
          </>
        )}
      </div>
    </div>
  )
}
