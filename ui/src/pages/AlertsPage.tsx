import { useMemo, useState } from 'react'
import clsx from 'clsx'
import * as Dialog from '@radix-ui/react-dialog'
import {
  Ban,
  Bell,
  CheckCircle2,
  Clock,
  Filter,
  Gauge,
  Hash,
  Mail,
  Terminal,
  TriangleAlert,
  Webhook,
} from 'lucide-react'
import {
  useAlertsConfig,
  useAlertDeliveries,
  useCurrentUser,
  useSnoozeRule,
  useDisableRule,
  useThrottleRule,
  useClearOverride,
} from '@/api/hooks'
import { Button } from '@/components/ui/button'
import { useConfirm } from '@/components/ui/confirm-dialog'
import { EmptyState } from '@/components/ui/empty-state'
import { RelativeTime } from '@/components/ui/relative-time'
import { Spinner } from '@/components/ui/spinner'
import { useToasts } from '@/lib/toast'
import { truncate } from '@/lib/utils'
import type {
  AlertChannelConfig,
  AlertChannelKind,
  AlertDelivery,
  AlertDeliveryState,
  AlertRuleConfig,
  AlertRuleOverride,
  AlertsConfig,
} from '@/api/types'

// ─── Page shell ──────────────────────────────────────────────────

type Tab = 'config' | 'deliveries'

export function AlertsPage() {
  const [tab, setTab] = useState<Tab>('config')

  return (
    <div className="page">
      <header className="page-header" style={{ padding: '14px 20px 0', borderBottom: '1px solid var(--border)' }}>
        <div className="row between" style={{ alignItems: 'flex-end' }}>
          <div className="col" style={{ gap: 2 }}>
            <h1 className="page-title row" style={{ alignItems: 'center', gap: 8 }}>
              <Bell size={18} /> Failure alerts
            </h1>
            <p className="dim" style={{ fontSize: 12.5, margin: 0 }}>
              Read-only view of the configured rules + channels and the recent delivery log.
              Rules and channels are managed in the Croniqfile&rsquo;s <code className="mono">alerts {'{ … }'}</code> block.
            </p>
          </div>
        </div>
        <div className="tabs" style={{ marginTop: 12 }}>
          {(['config', 'deliveries'] as Tab[]).map((id) => (
            <button
              key={id}
              type="button"
              className={clsx('tab', tab === id && 'active')}
              onClick={() => setTab(id)}
            >
              {id === 'config' ? 'Configuration' : 'Recent deliveries'}
            </button>
          ))}
        </div>
      </header>

      <div style={{ padding: 20 }}>
        {tab === 'config' ? <ConfigPane /> : <DeliveriesPane />}
      </div>
    </div>
  )
}

// ─── Configuration tab ──────────────────────────────────────────

function ConfigPane() {
  const { data, isLoading, isError } = useAlertsConfig()

  if (isLoading) {
    return (
      <div className="row center" style={{ padding: 30 }}>
        <Spinner className="h-5 w-5" />
      </div>
    )
  }
  if (isError || !data) {
    return (
      <EmptyState
        icon={<TriangleAlert className="h-8 w-8" />}
        title="Could not load alerts config"
        description="Check that the server is reachable and your token has alerts:read."
      />
    )
  }
  return <ConfigView config={data} />
}

function ConfigView({ config }: { config: AlertsConfig }) {
  const channels = useMemo(() => Object.values(config.channels), [config.channels])
  const rules = config.rules

  const { data: me } = useCurrentUser()
  const isAdmin = me?.role === 'admin'

  // Latest override per rule (server returns newest-set first; one row
  // per rule is the norm, but be defensive and keep the first seen).
  const overrideByRule = useMemo(() => {
    const m = new Map<string, AlertRuleOverride>()
    for (const ov of config.overrides ?? []) {
      if (!m.has(ov.rule_name)) m.set(ov.rule_name, ov)
    }
    return m
  }, [config.overrides])

  const { confirm, dialog: confirmDialog } = useConfirm()
  const toast = useToasts((s) => s.push)
  const clearOverride = useClearOverride()
  const [overrideTarget, setOverrideTarget] = useState<AlertRuleConfig | null>(null)

  async function handleClear(rule: AlertRuleConfig) {
    const ok = await confirm({
      title: `Clear override on ${rule.name}?`,
      description:
        'Removes the operational override and returns the rule to pure ' +
        'Croniqfile behaviour.',
      confirmLabel: 'Clear override',
    })
    if (!ok) return
    clearOverride.mutate(rule.name, {
      onSuccess: () => toast({ variant: 'success', message: `Override cleared on ${rule.name}` }),
    })
  }

  if (channels.length === 0 && rules.length === 0) {
    return (
      <EmptyState
        icon={<Bell className="h-10 w-10" />}
        title="No alerts configured"
        description={
          'Declare channels and rules in the Croniqfile alerts { … } block. ' +
          'See docs/operations.md for the syntax.'
        }
      />
    )
  }

  return (
    <div className="col" style={{ gap: 16 }}>
      <section className="card" style={{ padding: 0 }}>
        <header style={{ padding: '12px 16px', borderBottom: '1px solid var(--border)' }}>
          <div className="row between">
            <h2 className="card-title">Channels ({channels.length})</h2>
            <span className="dim" style={{ fontSize: 11.5 }}>
              Channels are referenced by name from rules.
            </span>
          </div>
        </header>
        {channels.length === 0 ? (
          <p className="dim" style={{ padding: 16, margin: 0, fontSize: 12.5 }}>
            No channels declared.
          </p>
        ) : (
          <ul className="reset-list" style={{ padding: 0, margin: 0 }}>
            {channels.map((ch) => (
              <ChannelRow key={ch.name} channel={ch} />
            ))}
          </ul>
        )}
      </section>

      <section className="card" style={{ padding: 0 }}>
        <header style={{ padding: '12px 16px', borderBottom: '1px solid var(--border)' }}>
          <div className="row between">
            <h2 className="card-title">Rules ({rules.length})</h2>
            <span className="dim" style={{ fontSize: 11.5 }}>
              {isAdmin
                ? 'Triggered by the evaluator; override snooze/disable/throttle below.'
                : 'Triggered by the evaluator; per-(rule, job_key) throttled.'}
            </span>
          </div>
        </header>
        {rules.length === 0 ? (
          <p className="dim" style={{ padding: 16, margin: 0, fontSize: 12.5 }}>
            No rules declared.
          </p>
        ) : (
          <ul className="reset-list" style={{ padding: 0, margin: 0 }}>
            {rules.map((r) => (
              <RuleRow
                key={r.name}
                rule={r}
                override={overrideByRule.get(r.name)}
                isAdmin={isAdmin}
                onOverride={() => setOverrideTarget(r)}
                onClear={() => handleClear(r)}
              />
            ))}
          </ul>
        )}
      </section>

      {overrideTarget && (
        <OverrideDialog rule={overrideTarget} onClose={() => setOverrideTarget(null)} />
      )}
      {confirmDialog}
    </div>
  )
}

function channelIcon(kind: AlertChannelKind) {
  switch (kind.type) {
    case 'shell':
      return <Terminal size={14} />
    case 'webhook':
      return <Webhook size={14} />
    case 'unknown':
      return <TriangleAlert size={14} style={{ color: 'var(--warn)' }} />
  }
}

function ChannelRow({ channel }: { channel: AlertChannelConfig }) {
  const { kind } = channel
  const detail = (() => {
    switch (kind.type) {
      case 'shell':
        return (
          <span className="mono dim ellipsis" title={kind.command} style={{ fontSize: 12 }}>
            {kind.command}
          </span>
        )
      case 'webhook':
        return (
          <span className="mono dim ellipsis" title={kind.url} style={{ fontSize: 12 }}>
            POST {kind.url}{' '}
            <span style={{ opacity: 0.7 }}>· timeout {kind.timeout_secs}s</span>
          </span>
        )
      case 'unknown':
        return (
          <span className="dim" style={{ fontSize: 12 }}>
            unrecognised channel kind — {kind.reason}
          </span>
        )
    }
  })()

  return (
    <li
      className="row between"
      style={{
        padding: '10px 16px',
        borderTop: '1px solid var(--border)',
        gap: 16,
      }}
    >
      <div className="row" style={{ gap: 10, minWidth: 0, alignItems: 'center' }}>
        {channelIcon(kind)}
        <span className="mono" style={{ fontWeight: 500 }}>
          {channel.name}
        </span>
        <span
          className="pill"
          style={{
            fontSize: 10.5,
            background: 'var(--panel)',
            border: '1px solid var(--border)',
            padding: '1px 6px',
          }}
        >
          {kind.type}
        </span>
      </div>
      <div style={{ minWidth: 0, flex: 1, textAlign: 'right' }}>{detail}</div>
    </li>
  )
}

function RuleRow({
  rule,
  override,
  isAdmin,
  onOverride,
  onClear,
}: {
  rule: AlertRuleConfig
  override?: AlertRuleOverride
  isAdmin: boolean
  onOverride: () => void
  onClear: () => void
}) {
  // An override whose deadline has passed is inert until the watchdog
  // sweeps it — render it as "expiring", not active.
  const active = override != null && isOverrideActive(override)

  return (
    <li
      className="col"
      style={{
        padding: '12px 16px',
        borderTop: '1px solid var(--border)',
        gap: 6,
      }}
    >
      <div className="row between">
        <div className="row" style={{ gap: 10, alignItems: 'center' }}>
          <span className="mono" style={{ fontWeight: 500 }}>
            {rule.name}
          </span>
          <span
            className="pill"
            style={{
              fontSize: 10.5,
              background: 'var(--panel)',
              border: '1px solid var(--border)',
              padding: '1px 6px',
            }}
          >
            {rule.trigger}
          </span>
          {active && <OverridePill override={override} />}
        </div>
        <div className="row" style={{ gap: 12, alignItems: 'center' }}>
          <div className="row mono dim" style={{ fontSize: 11.5, gap: 12 }}>
            {rule.channels.map((c) => (
              <span key={c} className="row" style={{ gap: 4, alignItems: 'center' }}>
                <Hash size={11} /> {c}
              </span>
            ))}
          </div>
          {isAdmin && (
            <div className="row" style={{ gap: 6 }}>
              <button type="button" className="btn xs ghost" onClick={onOverride}>
                {active ? 'Change' : 'Override'}
              </button>
              {override && (
                <button type="button" className="btn xs ghost" onClick={onClear}>
                  Clear
                </button>
              )}
            </div>
          )}
        </div>
      </div>
      <div className="row dim" style={{ fontSize: 11.5, gap: 14, flexWrap: 'wrap' }}>
        <span>
          job_key <code className="mono">{rule.job_key_glob}</code>
        </span>
        {rule.min_attempts > 1 ? <span>min_attempts {rule.min_attempts}</span> : null}
        {rule.dead_letter_only ? <span>dead_letter only</span> : null}
        {rule.throttle ? <span>throttle {rule.throttle}</span> : null}
        {rule.expected_within ? <span>expected_within {rule.expected_within}</span> : null}
      </div>
      {active && override && (
        <p className="dim" style={{ fontSize: 11.5, margin: 0 }}>
          “{override.note}” — set by{' '}
          <span className="mono">{override.set_by_user_id}</span>{' '}
          <RelativeTime iso={override.set_at} />
          {override.expires_at && (
            <>
              , clears <RelativeTime iso={override.expires_at} />
            </>
          )}
        </p>
      )}
    </li>
  )
}

/// True while an override is in force. A row with a past `expires_at`
/// (which equals `snooze_until` for snoozes) is inert — mirrors the
/// server's `effective_*` logic.
function isOverrideActive(ov: AlertRuleOverride): boolean {
  if (ov.expires_at == null) return true
  return new Date(ov.expires_at).getTime() > Date.now()
}

function formatThrottleSecs(secs: number): string {
  if (secs % 3600 === 0) return `${secs / 3600}h`
  if (secs % 60 === 0) return `${secs / 60}m`
  return `${secs}s`
}

function OverridePill({ override: ov }: { override: AlertRuleOverride }) {
  const { icon, label, color } = (() => {
    if (ov.enabled === false)
      return { icon: <Ban size={11} />, label: 'disabled', color: 'var(--error)' }
    if (ov.snooze_until != null)
      return { icon: <Clock size={11} />, label: 'snoozed', color: 'var(--warn)' }
    if (ov.throttle_secs != null)
      return {
        icon: <Gauge size={11} />,
        label: `throttle ${formatThrottleSecs(ov.throttle_secs)}`,
        color: 'var(--warn)',
      }
    return { icon: <TriangleAlert size={11} />, label: 'override', color: 'var(--warn)' }
  })()
  return (
    <span
      className="pill row"
      title="Operational override active"
      style={{
        gap: 4,
        alignItems: 'center',
        background: 'var(--panel)',
        color,
        border: `1px solid ${color}`,
        padding: '1px 7px',
        fontSize: 10.5,
        whiteSpace: 'nowrap',
      }}
    >
      {icon}
      {label}
    </span>
  )
}

// ─── Override dialog (admin-only) ────────────────────────────────

type OverrideMode = 'snooze' | 'disable' | 'throttle'

/// Converts a `datetime-local` value (local wall-clock, no zone) to an
/// RFC3339 UTC instant the server accepts. Empty → null.
function localToIso(v: string): string | null {
  if (!v) return null
  const d = new Date(v)
  return Number.isNaN(d.getTime()) ? null : d.toISOString()
}

function OverrideDialog({ rule, onClose }: { rule: AlertRuleConfig; onClose: () => void }) {
  const [mode, setMode] = useState<OverrideMode>('snooze')
  const [note, setNote] = useState('')
  const [until, setUntil] = useState('')
  const [throttle, setThrottle] = useState('')
  const [expiresAt, setExpiresAt] = useState('')
  const [err, setErr] = useState<string | null>(null)

  const toast = useToasts((s) => s.push)
  const snooze = useSnoozeRule()
  const disable = useDisableRule()
  const throttleMut = useThrottleRule()
  const pending = snooze.isPending || disable.isPending || throttleMut.isPending

  function done(verb: string) {
    toast({ variant: 'success', message: `${verb} ${rule.name}` })
    onClose()
  }

  function submit(e: React.FormEvent) {
    e.preventDefault()
    setErr(null)
    if (!note.trim()) {
      setErr('A note is required — capture why this override exists.')
      return
    }
    if (mode === 'snooze') {
      const iso = localToIso(until)
      if (!iso) {
        setErr('Pick a snooze deadline.')
        return
      }
      snooze.mutate(
        { name: rule.name, until: iso, note: note.trim() },
        { onSuccess: () => done('Snoozed') },
      )
    } else if (mode === 'disable') {
      disable.mutate(
        { name: rule.name, note: note.trim(), expires_at: localToIso(expiresAt) },
        { onSuccess: () => done('Disabled') },
      )
    } else {
      if (!throttle.trim()) {
        setErr('Enter a throttle duration, e.g. 30m or 1h.')
        return
      }
      throttleMut.mutate(
        {
          name: rule.name,
          throttle: throttle.trim(),
          note: note.trim(),
          expires_at: localToIso(expiresAt),
        },
        { onSuccess: () => done('Throttled') },
      )
    }
  }

  const inputCls =
    'w-full rounded-md border border-border bg-background px-2.5 py-1.5 text-sm ' +
    'focus:outline-none focus:ring-1 focus:ring-ring'

  return (
    <Dialog.Root open onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/40" />
        <Dialog.Content className="fixed left-1/2 top-1/2 z-50 w-full max-w-md -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-card p-6 shadow-xl">
          <Dialog.Title className="text-sm font-semibold">
            Override rule <span className="font-mono">{rule.name}</span>
          </Dialog.Title>
          <Dialog.Description className="mt-1 text-xs text-muted-foreground">
            Overwrites any existing override. Snooze, disable and throttle are
            distinct intents — only one is active at a time.
          </Dialog.Description>

          <form onSubmit={submit} className="mt-4 flex flex-col gap-3">
            <div className="flex gap-2">
              {(['snooze', 'disable', 'throttle'] as OverrideMode[]).map((m) => (
                <button
                  key={m}
                  type="button"
                  className={clsx('btn xs', mode === m ? 'primary' : 'ghost')}
                  onClick={() => {
                    setMode(m)
                    setErr(null)
                  }}
                >
                  {m}
                </button>
              ))}
            </div>

            {mode === 'snooze' && (
              <label className="flex flex-col gap-1 text-xs">
                <span className="text-muted-foreground">
                  Snooze until (also the auto-clear deadline)
                </span>
                <input
                  type="datetime-local"
                  className={inputCls}
                  value={until}
                  onChange={(e) => setUntil(e.target.value)}
                />
              </label>
            )}

            {mode === 'throttle' && (
              <label className="flex flex-col gap-1 text-xs">
                <span className="text-muted-foreground">
                  Throttle window (replaces the Croniqfile window)
                </span>
                <input
                  type="text"
                  className={inputCls}
                  placeholder="e.g. 30m, 1h, 90s"
                  value={throttle}
                  onChange={(e) => setThrottle(e.target.value)}
                />
              </label>
            )}

            {(mode === 'disable' || mode === 'throttle') && (
              <label className="flex flex-col gap-1 text-xs">
                <span className="text-muted-foreground">
                  Auto-clear at (optional — leave empty for open-ended)
                </span>
                <input
                  type="datetime-local"
                  className={inputCls}
                  value={expiresAt}
                  onChange={(e) => setExpiresAt(e.target.value)}
                />
              </label>
            )}

            <label className="flex flex-col gap-1 text-xs">
              <span className="text-muted-foreground">Note (required)</span>
              <textarea
                className={inputCls}
                rows={2}
                placeholder="Why is this override in place? e.g. INC-1234, noisy during migration"
                value={note}
                onChange={(e) => setNote(e.target.value)}
              />
            </label>

            {err && <p className="text-xs text-destructive">{err}</p>}

            <div className="mt-1 flex justify-end gap-2">
              <Button type="button" variant="secondary" size="sm" onClick={onClose}>
                Cancel
              </Button>
              <Button type="submit" variant="primary" size="sm" disabled={pending}>
                {pending ? 'Applying…' : `Apply ${mode}`}
              </Button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

// ─── Deliveries tab ──────────────────────────────────────────────

function DeliveriesPane() {
  const [stateFilter, setStateFilter] = useState<AlertDeliveryState | 'all'>('all')
  const [jobKey, setJobKey] = useState<string>('')

  const { data, isLoading } = useAlertDeliveries({
    state: stateFilter === 'all' ? undefined : stateFilter,
    job_key: jobKey.trim() || undefined,
    limit: 200,
  })

  return (
    <div className="col" style={{ gap: 12 }}>
      <div
        className="card row"
        style={{
          padding: '10px 14px',
          gap: 14,
          alignItems: 'center',
          flexWrap: 'wrap',
        }}
      >
        <div className="row" style={{ gap: 6, alignItems: 'center' }}>
          <Filter size={13} />
          <span className="dim" style={{ fontSize: 12 }}>
            Filter:
          </span>
        </div>
        <div className="row gap-6">
          {(['all', 'delivered', 'failed', 'throttled'] as const).map((s) => (
            <button
              key={s}
              type="button"
              className={clsx('btn xs', stateFilter === s ? 'primary' : 'ghost')}
              onClick={() => setStateFilter(s)}
            >
              {s}
            </button>
          ))}
        </div>
        <input
          type="text"
          className="input"
          placeholder="job_key (exact match)"
          value={jobKey}
          onChange={(e) => setJobKey(e.target.value)}
          style={{ minWidth: 200, flex: '1 1 240px' }}
        />
        <span className="dim mono" style={{ fontSize: 11.5 }}>
          {data?.length ?? 0} row{data?.length === 1 ? '' : 's'}
        </span>
      </div>

      <DeliveriesList rows={data ?? []} isLoading={isLoading} />
    </div>
  )
}

export function DeliveriesList({ rows, isLoading }: { rows: AlertDelivery[]; isLoading: boolean }) {
  if (isLoading) {
    return (
      <div className="row center" style={{ padding: 30 }}>
        <Spinner className="h-5 w-5" />
      </div>
    )
  }
  if (rows.length === 0) {
    return (
      <EmptyState
        icon={<Bell className="h-10 w-10" />}
        title="No deliveries yet"
        description="Once an alert rule fires, you&rsquo;ll see the per-channel delivery rows here."
      />
    )
  }
  return (
    <div className="card" style={{ padding: 0 }}>
      <table className="tbl" style={{ width: '100%', borderCollapse: 'collapse' }}>
        <thead>
          <tr>
            <th style={{ textAlign: 'left', padding: '8px 12px', fontSize: 11, fontWeight: 500 }}>
              When
            </th>
            <th style={{ textAlign: 'left', padding: '8px 12px', fontSize: 11, fontWeight: 500 }}>
              Job
            </th>
            <th style={{ textAlign: 'left', padding: '8px 12px', fontSize: 11, fontWeight: 500 }}>
              Rule
            </th>
            <th style={{ textAlign: 'left', padding: '8px 12px', fontSize: 11, fontWeight: 500 }}>
              Channel
            </th>
            <th style={{ textAlign: 'left', padding: '8px 12px', fontSize: 11, fontWeight: 500 }}>
              State
            </th>
            <th style={{ textAlign: 'left', padding: '8px 12px', fontSize: 11, fontWeight: 500 }}>
              Notes
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <DeliveryRow key={r.delivery_id} delivery={r} />
          ))}
        </tbody>
      </table>
    </div>
  )
}

export function DeliveryRow({ delivery: r }: { delivery: AlertDelivery }) {
  return (
    <tr style={{ borderTop: '1px solid var(--border)' }}>
      <td className="mono dim" style={{ padding: '8px 12px', fontSize: 11.5, whiteSpace: 'nowrap' }}>
        <RelativeTime iso={r.fired_at} />
      </td>
      <td className="mono" style={{ padding: '8px 12px', fontSize: 12 }}>
        {r.job_key}
      </td>
      <td className="mono" style={{ padding: '8px 12px', fontSize: 12 }}>
        {r.rule_name}
      </td>
      <td className="mono" style={{ padding: '8px 12px', fontSize: 12 }}>
        {r.channel_name}
      </td>
      <td style={{ padding: '8px 12px' }}>
        <StatePill state={r.state} />
      </td>
      <td className="dim" style={{ padding: '8px 12px', fontSize: 12, maxWidth: 320 }}>
        {r.error ? truncate(r.error, 120) : r.state === 'delivered' ? 'OK' : ''}
      </td>
    </tr>
  )
}

function StatePill({ state }: { state: AlertDeliveryState }) {
  const { color, bg, icon, label } = (() => {
    switch (state) {
      case 'delivered':
        return {
          color: 'var(--success)',
          bg: 'var(--success-bg)',
          icon: <CheckCircle2 size={11} />,
          label: 'delivered',
        }
      case 'failed':
        return {
          color: 'var(--error)',
          bg: 'var(--error-bg)',
          icon: <TriangleAlert size={11} />,
          label: 'failed',
        }
      case 'throttled':
        return {
          color: 'var(--warn)',
          bg: 'var(--panel)',
          icon: <Mail size={11} />,
          label: 'throttled',
        }
    }
  })()
  return (
    <span
      className="pill row"
      style={{
        gap: 4,
        alignItems: 'center',
        background: bg,
        color,
        padding: '1px 8px',
        border: `1px solid ${color}`,
        fontSize: 11,
        whiteSpace: 'nowrap',
      }}
    >
      {icon}
      {label}
    </span>
  )
}
