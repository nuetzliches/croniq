import { useMemo } from 'react'
import { Link } from 'react-router'
import { ArrowUpRight, Cpu, Activity, BarChart2 } from 'lucide-react'
import {
  useHealth,
  useJobs,
  useThroughput,
  useFailureHeatmap,
  useExecutions,
  useRunners,
  useDeadLetters,
} from '@/api/hooks'
import {
  KPICard,
  Sparkline,
  Donut,
  EmptyState,
  StatusPill,
  HeatCell,
} from '@/components/primitives'
import { formatRelative } from '@/lib/utils'
import type { Execution, ThroughputBucket, RunnerSummary } from '@/api/types'

export function DashboardPage() {
  const health = useHealth()
  const jobs = useJobs()
  const runners = useRunners()
  const deadLetters = useDeadLetters()
  const throughput = useThroughput('24h')
  const heatmap = useFailureHeatmap(7)
  const recentExecutions = useExecutions({ limit: 8 })

  const okSeries = useMemo(
    () => (throughput.data?.buckets ?? []).map((b) => b.ok),
    [throughput.data],
  )
  const errSeries = useMemo(
    () => (throughput.data?.buckets ?? []).map((b) => b.err),
    [throughput.data],
  )
  const totalOk24h = okSeries.reduce((a, b) => a + b, 0)
  const totalErr24h = errSeries.reduce((a, b) => a + b, 0)
  const total = totalOk24h + totalErr24h
  const successRate = total === 0 ? null : (totalOk24h / total) * 100

  const queueDepth = jobs.data?.filter((j) => j.is_active).length ?? 0
  const runnersOnline = (runners.data ?? []).filter((r) => r.status === 'online').length
  const runnersTotal = runners.data?.length ?? 0
  const deadCount = deadLetters.data?.length ?? 0

  // Heatmap → flatten to a 24-cell-wide grid where each row is one day
  // (oldest first, newest last). Cap intensity to the 95th percentile so
  // one extreme outlier doesn't wash out the rest.
  const heatPeak = useMemo(() => {
    const vals = (heatmap.data?.rows ?? []).flat().filter((v) => v > 0)
    if (vals.length === 0) return 1
    vals.sort((a, b) => a - b)
    return Math.max(1, vals[Math.floor(vals.length * 0.95)] ?? 1)
  }, [heatmap.data])

  return (
    <div className="page wide">
      <div className="page-head">
        <div>
          <h1 className="page-title">Dashboard</h1>
          <p className="page-subtitle">
            Health, throughput and reliability across all jobs.
          </p>
        </div>
        <Link to="/jobs" className="btn primary">
          Browse jobs <ArrowUpRight size={14} />
        </Link>
      </div>

      <div className="grid cols-4">
        <KPICard
          title="Queue depth"
          value={queueDepth}
          sub={<span>{jobs.data?.length ?? 0} jobs total</span>}
          icon={
            <span className="dot-status idle" aria-hidden style={{ width: 10, height: 10 }} />
          }
        />
        <KPICard
          title="Runners online"
          value={runnersOnline}
          sub={
            <span className={runnersOnline === runnersTotal ? 'muted' : ''}>
              {runnersOnline === runnersTotal && runnersTotal > 0
                ? 'all healthy'
                : `${runnersTotal} total`}
            </span>
          }
          icon={
            <span
              className={`dot-status ${
                runnersOnline === 0
                  ? 'error'
                  : runnersOnline < runnersTotal
                    ? 'warn'
                    : 'success'
              }`}
              aria-hidden
              style={{ width: 10, height: 10 }}
            />
          }
        />
        <KPICard
          title="Success rate (24h)"
          value={successRate === null ? '—' : `${successRate.toFixed(1)}%`}
          sub={
            total > 0 ? (
              <span>
                {totalOk24h.toLocaleString()} ok · {totalErr24h.toLocaleString()} err
              </span>
            ) : (
              <span>No executions in window</span>
            )
          }
          chart={total > 0 ? <Sparkline data={okSeries} height={32} /> : null}
        />
        <KPICard
          title="Dead letters"
          value={deadCount}
          sub={
            deadCount > 0 ? (
              <Link to="/dead-letters" className="kpi-delta down">
                View →
              </Link>
            ) : (
              <span className="muted">none pending</span>
            )
          }
          icon={
            <span
              className={`dot-status ${deadCount > 0 ? 'error' : 'success'}`}
              aria-hidden
              style={{ width: 10, height: 10 }}
            />
          }
        />
      </div>

      <div className="grid cols-2" style={{ marginTop: 14 }}>
        <ThroughputCard ok={okSeries} err={errSeries} loading={throughput.isLoading} />
        <HeatmapCard
          rows={heatmap.data?.rows ?? []}
          days={heatmap.data?.days ?? 7}
          peak={heatPeak}
          loading={heatmap.isLoading}
        />
      </div>

      <div className="grid cols-2" style={{ marginTop: 14 }}>
        <ActivityCard executions={recentExecutions.data ?? []} loading={recentExecutions.isLoading} />
        <RunnerFleetCard runners={runners.data ?? []} loading={runners.isLoading} />
      </div>

      {health.data && health.data.status !== 'ok' ? (
        <div className="banner warn" style={{ marginTop: 14 }} role="status">
          <span className="grow">
            Backend health: <strong>{health.data.status}</strong>
          </span>
        </div>
      ) : null}
    </div>
  )
}

function ThroughputCard({
  ok,
  err,
  loading,
}: {
  ok: number[]
  err: number[]
  loading: boolean
}) {
  const okSum = ok.reduce((a, b) => a + b, 0)
  const errSum = err.reduce((a, b) => a + b, 0)
  return (
    <section className="card">
      <div className="card-head">
        <p className="card-title">Throughput · last 24h</p>
        <span className="dim row gap-6" style={{ fontSize: 11.5 }}>
          <BarChart2 size={12} /> hourly buckets
        </span>
      </div>
      {loading ? (
        <div style={{ height: 76 }} className="dim center">
          Loading…
        </div>
      ) : ok.length === 0 ? (
        <EmptyState icon={Activity} title="No data yet" desc="Trigger a job to populate this chart." />
      ) : (
        <div className="col" style={{ gap: 6 }}>
          <div className="row between" style={{ fontSize: 12 }}>
            <span className="dim">ok</span>
            <span className="mono tnum" style={{ color: 'var(--success)' }}>
              {okSum.toLocaleString()}
            </span>
          </div>
          <Sparkline data={ok} color="var(--success)" height={36} />
          <div className="row between" style={{ fontSize: 12, marginTop: 6 }}>
            <span className="dim">err</span>
            <span className="mono tnum" style={{ color: 'var(--error)' }}>
              {errSum.toLocaleString()}
            </span>
          </div>
          <Sparkline data={err.length === 0 ? ok.map(() => 0) : err} color="var(--error)" height={28} />
        </div>
      )}
    </section>
  )
}

function HeatmapCard({
  rows,
  days,
  peak,
  loading,
}: {
  rows: number[][]
  days: number
  peak: number
  loading: boolean
}) {
  return (
    <section className="card">
      <div className="card-head">
        <p className="card-title">Failures · last {days}d</p>
        <span className="dim" style={{ fontSize: 11.5 }}>
          day × hour
        </span>
      </div>
      {loading ? (
        <div style={{ height: 100 }} className="dim center">
          Loading…
        </div>
      ) : rows.length === 0 ? (
        <EmptyState icon={Activity} title="No failures recorded" desc="Smooth sailing so far." />
      ) : (
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(24, 1fr)',
            gap: 2,
          }}
        >
          {rows.flatMap((row, dIdx) =>
            row.map((v, hIdx) => (
              <HeatCell
                key={`${dIdx}-${hIdx}`}
                value={v}
                max={peak}
                title={`Day -${rows.length - 1 - dIdx}, ${hIdx}:00 — ${v} failures`}
              />
            )),
          )}
        </div>
      )}
    </section>
  )
}

function ActivityCard({ executions, loading }: { executions: Execution[]; loading: boolean }) {
  return (
    <section className="card" style={{ padding: 0 }}>
      <div className="row between" style={{ padding: 16 }}>
        <p className="card-title">Recent executions</p>
        <Link to="/executions" className="dim" style={{ fontSize: 12, textDecoration: 'none' }}>
          View all →
        </Link>
      </div>
      <div style={{ borderTop: '1px solid var(--divider)', padding: 6 }}>
        {loading ? (
          <div className="dim center" style={{ padding: 20 }}>
            Loading…
          </div>
        ) : executions.length === 0 ? (
          <EmptyState icon={Activity} title="No executions yet" desc="Trigger a job to see runs here." />
        ) : (
          executions.slice(0, 8).map((e) => (
            <div
              key={e.id}
              className="row"
              style={{
                padding: '8px 10px',
                gap: 10,
                fontSize: 12.5,
                borderRadius: 'var(--r-2)',
              }}
            >
              <StatusPill state={e.state} />
              <span className="ellipsis grow mono" style={{ fontSize: 12 }}>{e.job_key}</span>
              {e.duration_ms != null ? (
                <span className="dim mono tnum" style={{ fontSize: 11 }}>{e.duration_ms}ms</span>
              ) : null}
              <span className="dim mono tnum" style={{ fontSize: 11 }}>
                {formatRelative(e.fire_at)}
              </span>
            </div>
          ))
        )}
      </div>
    </section>
  )
}

function RunnerFleetCard({
  runners,
  loading,
}: {
  runners: RunnerSummary[]
  loading: boolean
}) {
  return (
    <section className="card" style={{ padding: 0 }}>
      <div className="row between" style={{ padding: 16 }}>
        <p className="card-title">Runner fleet</p>
        <Link to="/runners" className="dim" style={{ fontSize: 12, textDecoration: 'none' }}>
          Manage →
        </Link>
      </div>
      <div style={{ borderTop: '1px solid var(--divider)' }}>
        {loading ? (
          <div className="dim center" style={{ padding: 20 }}>
            Loading…
          </div>
        ) : runners.length === 0 ? (
          <EmptyState
            icon={Cpu}
            title="No runners registered"
            desc="Register a runner with `croniq-runner` to start executing jobs."
          />
        ) : (
          runners.slice(0, 6).map((r) => (
            <div
              key={r.runner_id}
              className="row"
              style={{
                padding: '10px 16px',
                gap: 12,
                borderBottom: '1px solid var(--divider)',
              }}
            >
              <Donut value={r.inflight} max={Math.max(r.max_inflight, 1)} size={32} thickness={3} />
              <div className="col" style={{ gap: 2, flex: 1, minWidth: 0 }}>
                <span className="mono ellipsis" style={{ fontSize: 12.5, color: 'var(--fg)' }}>
                  {r.runner_id}
                </span>
                <span className="dim mono" style={{ fontSize: 11 }}>
                  {r.tags.length > 0 ? r.tags.join(' · ') : 'no tags'}
                </span>
              </div>
              <StatusPill state={r.status} />
            </div>
          ))
        )}
      </div>
    </section>
  )
}

// Unused export prevented by tree-shaking but kept for backwards compat
// imports from older test files.
export type { ThroughputBucket }
