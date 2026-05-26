import { useMemo, useState } from 'react'
import clsx from 'clsx'
import {
  Bell,
  CheckCircle2,
  Filter,
  Hash,
  Mail,
  Terminal,
  TriangleAlert,
  Webhook,
} from 'lucide-react'
import { useAlertsConfig, useAlertDeliveries } from '@/api/hooks'
import { EmptyState } from '@/components/ui/empty-state'
import { RelativeTime } from '@/components/ui/relative-time'
import { Spinner } from '@/components/ui/spinner'
import { truncate } from '@/lib/utils'
import type {
  AlertChannelConfig,
  AlertChannelKind,
  AlertDelivery,
  AlertDeliveryState,
  AlertRuleConfig,
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
              Triggered by the evaluator; per-(rule, job_key) throttled.
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
              <RuleRow key={r.name} rule={r} />
            ))}
          </ul>
        )}
      </section>
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

function RuleRow({ rule }: { rule: AlertRuleConfig }) {
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
        </div>
        <div className="row mono dim" style={{ fontSize: 11.5, gap: 12 }}>
          {rule.channels.map((c) => (
            <span key={c} className="row" style={{ gap: 4, alignItems: 'center' }}>
              <Hash size={11} /> {c}
            </span>
          ))}
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
    </li>
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
