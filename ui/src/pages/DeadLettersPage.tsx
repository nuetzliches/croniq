import { Fragment, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { TriangleAlert, RotateCcw, Trash2, MailX, CheckCircle2, MousePointerClick } from 'lucide-react'
import { useDeadLetters, useDeleteDeadLetter, useBulkDeleteDeadLetters, useDeadLetter, useReplayDeadLetter, useJob } from '@/api/hooks'
import { EmptyState } from '@/components/ui/empty-state'
import { Spinner } from '@/components/ui/spinner'
import { CopyButton } from '@/components/ui/copy-button'
import { RelativeTime } from '@/components/ui/relative-time'
import { useConfirm } from '@/components/ui/confirm-dialog'
import { formatDate, truncate } from '@/lib/utils'
import { ExecutionLink } from '@/components/entity-links'

// The job's `description` (and any `operator_hint` baked into dead_reason)
// is the fastest path from "this failed" to "here's what to do". The job
// may have been deleted since the letter landed, so a 404 is silent.
function JobDescription({ jobKey }: { jobKey: string }) {
  const { data } = useJob(jobKey)
  if (!data?.description) return null
  return (
    <span className="dim" style={{ fontSize: 12.5, color: 'var(--fg-1)' }}>
      {data.description}
    </span>
  )
}

function DeadLetterDetail({
  id,
  onReplay,
  onDelete,
}: {
  id: string
  onReplay: (result: { execution_id: string; attempt: number }) => void
  onClose: () => void
  onDelete: () => void
}) {
  const { data, isLoading } = useDeadLetter(id)
  const replay = useReplayDeadLetter()

  if (isLoading) return <div className="flex justify-center py-8"><Spinner className="h-5 w-5" /></div>
  if (!data) return null

  async function handleReplay() {
    if (!data) return
    const result = await replay.mutateAsync(data.id)
    onReplay(result)
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 'var(--card-gap)' }}>
      <div className="card" style={{ padding: '16px 20px' }}>
        <div className="row between" style={{ marginBottom: 12, gap: 12, flexWrap: 'wrap' }}>
          <div className="col" style={{ gap: 4, minWidth: 0, flex: '1 1 280px' }}>
            <Link
              to={`/jobs/${encodeURIComponent(data.job_key)}`}
              className="mono ellipsis"
              style={{ margin: 0, fontSize: 18, fontWeight: 600, letterSpacing: '-0.01em', color: 'var(--fg)', textDecoration: 'none' }}
              title={`Open job ${data.job_key}`}
            >
              {data.job_key}
            </Link>
            <span className="dim" style={{ fontSize: 12 }}>{data.dead_reason} · attempt {data.attempt}</span>
            <JobDescription jobKey={data.job_key} />
          </div>
          <div className="row gap-6">
            <button
              type="button"
              className="btn sm primary"
              onClick={handleReplay}
              disabled={replay.isPending}
            >
              {replay.isPending ? <Spinner className="h-3.5 w-3.5" /> : <RotateCcw size={13} />} Replay
            </button>
            <button
              type="button"
              className="btn sm ghost"
              onClick={onDelete}
              title="Delete dead letter"
            >
              <Trash2 size={13} /> Delete
            </button>
          </div>
        </div>
        <div className="grid" style={{ gridTemplateColumns: 'auto 1fr', columnGap: 16, rowGap: 6, fontSize: 12.5 }}>
          {[
            ['Execution', (
              <span key="x" className="row mono" style={{ gap: 6, alignItems: 'center' }}>
                <ExecutionLink id={data.execution_id} />
                <CopyButton value={data.execution_id} label="Copy execution id" />
              </span>
            )],
            ['Created', formatDate(data.created_at)],
            ['Expires', data.expires_at ? formatDate(data.expires_at) : '—'],
          ].map(([label, value]) => (
            <Fragment key={String(label)}>
              <span className="dim">{label}</span>
              <span style={{ minWidth: 0, overflow: 'hidden' }}>{value}</span>
            </Fragment>
          ))}
        </div>
      </div>

      <div className="card" style={{ padding: 0 }}>
        <div style={{ padding: '12px 20px', borderBottom: '1px solid var(--border)' }}>
          <p className="card-title">Error</p>
        </div>
        <pre
          className="mono"
          style={{
            margin: 0,
            padding: '14px 20px',
            fontSize: 12,
            color: 'var(--fg-1)',
            whiteSpace: 'pre-wrap',
            lineHeight: 1.55,
            overflow: 'auto',
            maxHeight: 320,
          }}
        >
          {data.error}
        </pre>
      </div>

      {Object.keys(data.metadata).length > 0 && (
        <div className="card" style={{ padding: 0 }}>
          <div style={{ padding: '12px 20px', borderBottom: '1px solid var(--border)' }}>
            <p className="card-title">Metadata</p>
          </div>
          <pre
            className="mono"
            style={{
              margin: 0,
              padding: '14px 20px',
              fontSize: 12,
              color: 'var(--fg-1)',
              whiteSpace: 'pre-wrap',
              lineHeight: 1.55,
              overflow: 'auto',
            }}
          >
            {JSON.stringify(data.metadata, null, 2)}
          </pre>
        </div>
      )}
    </div>
  )
}

export function DeadLettersPage() {
  const { data: items, isLoading } = useDeadLetters()
  const deleteDL = useDeleteDeadLetter()
  const bulkDelete = useBulkDeleteDeadLetters()
  const { confirm, dialog: confirmDialog } = useConfirm()
  const { id: routeId } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const selectedId = routeId ?? null
  const setSelectedId = (id: string | null) => {
    navigate(id ? `/dead-letters/${id}` : '/dead-letters', { replace: false })
  }
  // The dead-letter row vanishes after replay (the server invalidates
  // and our `dead-letters` query refetches), so we surface the new
  // execution_id at the page level — otherwise the user would never see
  // what they just spawned.
  const [lastReplay, setLastReplay] = useState<{
    job_key: string
    execution_id: string
    attempt: number
  } | null>(null)

  async function handleRowDelete(dl: { id: string; job_key: string }) {
    const ok = await confirm({
      title: `Delete dead letter for ${dl.job_key}?`,
      description:
        'The failed execution record will be removed permanently. The job itself is unaffected.',
      confirmLabel: 'Delete',
      destructive: true,
    })
    if (ok) {
      deleteDL.mutate(dl.id)
      if (selectedId === dl.id) setSelectedId(null)
    }
  }

  async function handleClearAll() {
    const count = items?.length ?? 0
    if (count === 0) return
    const ok = await confirm({
      title: `Delete all ${count} dead letters?`,
      description:
        'Every pending dead letter will be removed permanently. The jobs themselves are unaffected. This cannot be undone.',
      confirmLabel: 'Delete all',
      destructive: true,
    })
    if (ok) {
      await bulkDelete.mutateAsync({ all: true })
      setSelectedId(null)
    }
  }

  const selectedDL = items?.find((i) => i.id === selectedId) ?? null

  return (
    <div className="split">
      {confirmDialog}
      <aside className="master" aria-label="Dead letters list">
        <div className="master-filter" style={{ padding: '12px 14px' }}>
          <div className="row between">
            <span className="mono dim" style={{ fontSize: 12 }}>
              {items?.length ?? 0} pending
            </span>
            <button
              type="button"
              className="btn sm ghost"
              onClick={handleClearAll}
              disabled={(items?.length ?? 0) === 0 || bulkDelete.isPending}
              title="Delete all dead letters"
            >
              {bulkDelete.isPending ? <Spinner className="h-3.5 w-3.5" /> : <Trash2 size={13} />} Clear all
            </button>
          </div>
          {lastReplay && (
            <div
              className="row"
              style={{
                marginTop: 10,
                gap: 8,
                padding: '8px 10px',
                borderRadius: 'var(--r-2)',
                background: 'var(--success-bg)',
                border: '1px solid var(--success)',
                fontSize: 12,
                alignItems: 'flex-start',
              }}
            >
              <CheckCircle2 className="h-3.5 w-3.5 shrink-0 mt-0.5" style={{ color: 'var(--success)' }} />
              <div className="grow" style={{ minWidth: 0 }}>
                <p style={{ margin: 0, color: 'var(--success)' }}>
                  Replay queued for {lastReplay.job_key} · attempt {lastReplay.attempt}
                </p>
                <p className="row mono dim" style={{ margin: '2px 0 0', gap: 4, alignItems: 'center' }}>
                  <span className="ellipsis"><ExecutionLink id={lastReplay.execution_id} /></span>
                  <CopyButton value={lastReplay.execution_id} label="Copy new execution id" />
                </p>
              </div>
              <button
                onClick={() => setLastReplay(null)}
                aria-label="Dismiss replay confirmation"
                className="btn icon sm ghost"
                style={{ padding: 0 }}
              >
                <span aria-hidden="true">×</span>
              </button>
            </div>
          )}
        </div>

        <div className="master-list">
          {isLoading ? (
            <div className="dim center" style={{ padding: 30 }}>Loading…</div>
          ) : (items?.length ?? 0) === 0 ? (
            <EmptyState
              icon={<MailX className="h-10 w-10" />}
              title="No dead letters"
              description="Failed executions that exhaust retries appear here"
            />
          ) : (
            items?.map((dl) => {
              const active = dl.id === selectedId
              return (
                <button
                  key={dl.id}
                  type="button"
                  className={`job-row${active ? ' active' : ''}`}
                  onClick={() => setSelectedId(dl.id)}
                >
                  <div className="row between">
                    <span className="key ellipsis row" style={{ minWidth: 0, flex: 1, gap: 6, alignItems: 'center' }}>
                      {dl.dead_reason === 'max_retries_exceeded' && (
                        <TriangleAlert size={13} style={{ color: 'var(--error)', flexShrink: 0 }} aria-label="Max retries exceeded" />
                      )}
                      <span className="ellipsis">{dl.job_key}</span>
                    </span>
                    <span className="dim mono" style={{ fontSize: 10.5, flexShrink: 0 }}>
                      attempt {dl.attempt}
                    </span>
                  </div>
                  <div className="dim ellipsis" style={{ fontSize: 11.5 }}>
                    {truncate(dl.error, 80)}
                  </div>
                  <div className="row between">
                    <span className="dim mono" style={{ fontSize: 10.5 }}>
                      <RelativeTime iso={dl.created_at} />
                    </span>
                  </div>
                </button>
              )
            })
          )}
        </div>
      </aside>

      <section className="detail" aria-label="Dead letter detail">
        {selectedDL ? (
          <DeadLetterDetail
            id={selectedDL.id}
            onReplay={(result) => {
              setLastReplay({ job_key: selectedDL.job_key, ...result })
              setSelectedId(null)
            }}
            onClose={() => setSelectedId(null)}
            onDelete={() => handleRowDelete(selectedDL)}
          />
        ) : (
          <EmptyState
            icon={<MousePointerClick className="h-10 w-10" />}
            title="Select a dead letter"
            description="Pick an entry on the left to see the failure envelope, error and metadata."
          />
        )}
      </section>
    </div>
  )
}
