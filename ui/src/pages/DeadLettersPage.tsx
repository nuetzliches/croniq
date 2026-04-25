import { Fragment, useState } from 'react'
import { TriangleAlert, RotateCcw, Trash2, MailX, CheckCircle2 } from 'lucide-react'
import { useDeadLetters, useDeleteDeadLetter, useDeadLetter, useReplayDeadLetter } from '@/api/hooks'
import { Sheet } from '@/components/ui/sheet'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/ui/empty-state'
import { Spinner } from '@/components/ui/spinner'
import { CopyButton } from '@/components/ui/copy-button'
import { RelativeTime } from '@/components/ui/relative-time'
import { useConfirm } from '@/components/ui/confirm-dialog'
import { formatDate, truncate } from '@/lib/utils'

function DeadLetterDetail({
  id,
  onReplay,
  onClose,
}: {
  id: string
  onReplay: (result: { execution_id: string; attempt: number }) => void
  onClose: () => void
}) {
  const { data, isLoading } = useDeadLetter(id)
  const replay = useReplayDeadLetter()
  const del = useDeleteDeadLetter()
  const { confirm, dialog: confirmDialog } = useConfirm()

  if (isLoading) return <div className="flex justify-center py-8"><Spinner className="h-5 w-5" /></div>
  if (!data) return null

  async function handleReplay() {
    if (!data) return
    const result = await replay.mutateAsync(data.id)
    onReplay(result)
    onClose()
  }

  async function handleDelete() {
    if (!data) return
    const ok = await confirm({
      title: 'Delete dead letter?',
      description:
        'The failed execution record will be removed permanently. The job itself is unaffected — only the dead-letter entry is purged.',
      confirmLabel: 'Delete',
      destructive: true,
    })
    if (ok) {
      del.mutate(data.id)
      onClose()
    }
  }

  return (
    <div className="space-y-4">
      {confirmDialog}
      <div className="grid grid-cols-2 gap-x-4 gap-y-1.5 text-xs">
        {[
          ['Job', data.job_key],
          ['Attempt', data.attempt],
          ['Reason', data.dead_reason],
          ['Execution ID', (
            <span key="x" className="flex items-center gap-1.5 font-mono">
              {data.execution_id}
              <CopyButton value={data.execution_id} label="Copy execution id" />
            </span>
          )],
          ['Created', formatDate(data.created_at)],
          ['Expires', data.expires_at ? formatDate(data.expires_at) : '—'],
        ].map(([label, value]) => (
          // Plain `<>` fragments don't accept `key`; React warns when one
          // element of a list is rendered through them. The label/value
          // pair sits inside a CSS grid, so we need the two spans as
          // siblings — Fragment with a key gives us both.
          <Fragment key={String(label)}>
            <span className="text-muted-foreground">{label}</span>
            <span className="font-medium text-foreground">{value}</span>
          </Fragment>
        ))}
      </div>

      <div>
        <p className="text-xs text-muted-foreground mb-1">Error</p>
        <pre className="text-xs bg-muted rounded-md p-3 overflow-auto max-h-40 whitespace-pre-wrap">{data.error}</pre>
      </div>

      {Object.keys(data.metadata).length > 0 && (
        <div>
          <p className="text-xs text-muted-foreground mb-1">Metadata</p>
          <pre className="text-xs bg-muted rounded-md p-3 overflow-auto">{JSON.stringify(data.metadata, null, 2)}</pre>
        </div>
      )}

      <div className="flex gap-2 pt-2">
        <Button
          size="sm"
          onClick={handleReplay}
          disabled={replay.isPending}
        >
          {replay.isPending ? <Spinner className="h-3.5 w-3.5" /> : <RotateCcw className="h-3.5 w-3.5" />}
          Replay
        </Button>
        <Button
          size="sm"
          variant="destructive"
          onClick={handleDelete}
          disabled={del.isPending}
        >
          <Trash2 className="h-3.5 w-3.5" />
          Delete
        </Button>
      </div>
    </div>
  )
}

export function DeadLettersPage() {
  const { data: items, isLoading } = useDeadLetters()
  const deleteDL = useDeleteDeadLetter()
  const replay = useReplayDeadLetter()
  const { confirm, dialog: confirmDialog } = useConfirm()
  const [selectedId, setSelectedId] = useState<string | null>(null)
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
    if (ok) deleteDL.mutate(dl.id)
  }

  async function handleRowReplay(dl: { id: string; job_key: string }) {
    const result = await replay.mutateAsync(dl.id)
    setLastReplay({ job_key: dl.job_key, ...result })
  }

  return (
    <div className="space-y-4">
      {confirmDialog}
      {lastReplay && (
        <div className="rounded-md border border-status-ok-fg/40 bg-status-ok-fg/10 p-3 text-xs flex items-start gap-3">
          <CheckCircle2 className="h-4 w-4 text-status-ok-fg shrink-0 mt-0.5" />
          <div className="flex-1 min-w-0">
            <p className="font-medium text-status-ok-fg">
              Replay queued for {lastReplay.job_key} (attempt {lastReplay.attempt})
            </p>
            <p className="flex items-center gap-1.5 font-mono text-foreground mt-0.5">
              <span className="truncate">New execution: {lastReplay.execution_id}</span>
              <CopyButton value={lastReplay.execution_id} label="Copy new execution id" />
            </p>
          </div>
          <button
            onClick={() => setLastReplay(null)}
            aria-label="Dismiss replay confirmation"
            className="text-muted-foreground hover:text-foreground p-0.5"
          >
            <span aria-hidden="true">×</span>
          </button>
        </div>
      )}
      {isLoading && <div className="flex justify-center py-12"><Spinner className="h-6 w-6" /></div>}

      {!isLoading && items?.length === 0 && (
        <EmptyState
          icon={<MailX className="h-10 w-10" />}
          title="No dead letters"
          description="Failed executions that exhaust retries appear here"
        />
      )}

      {(items?.length ?? 0) > 0 && (
        <div className="rounded-lg border border-border bg-card overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border">
                {['Job', 'Attempt', 'Error', 'Created', ''].map((h, i) => (
                  <th key={i} className="px-3 py-2.5 text-left text-xs font-medium text-muted-foreground uppercase tracking-wide">{h}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {items?.map((dl) => (
                <tr
                  key={dl.id}
                  onClick={() => setSelectedId(dl.id)}
                  className={`border-b border-border last:border-0 cursor-pointer transition-colors hover:bg-accent/40 ${selectedId === dl.id ? 'bg-accent/60' : ''}`}
                >
                  <td className="px-3 py-2.5">
                    <span className="flex items-center gap-1.5 font-mono text-xs">
                      {dl.dead_reason === 'max_retries_exceeded' && (
                        <TriangleAlert className="h-3.5 w-3.5 text-destructive shrink-0" aria-label="Max retries exceeded" />
                      )}
                      {dl.job_key}
                    </span>
                  </td>
                  <td className="px-3 py-2.5 text-muted-foreground">{dl.attempt}</td>
                  <td className="px-3 py-2.5 text-muted-foreground">{truncate(dl.error, 50)}</td>
                  <td className="px-3 py-2.5 text-muted-foreground">
                    <RelativeTime iso={dl.created_at} />
                  </td>
                  <td className="px-3 py-2.5">
                    <div className="flex items-center justify-end gap-1" onClick={(e) => e.stopPropagation()}>
                      <Button
                        variant="ghost" size="sm"
                        onClick={() => handleRowReplay(dl)}
                        disabled={replay.isPending}
                        aria-label="Replay"
                        title="Replay"
                        className="h-7 w-7 p-0"
                      >
                        <RotateCcw className="h-3.5 w-3.5" />
                      </Button>
                      <Button
                        variant="ghost" size="sm"
                        onClick={() => handleRowDelete(dl)}
                        aria-label="Delete"
                        className="h-7 w-7 p-0 hover:text-destructive"
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </Button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <Sheet open={!!selectedId} onOpenChange={(o) => !o && setSelectedId(null)} title="Dead Letter Detail">
        {selectedId && (() => {
          const current = items?.find((i) => i.id === selectedId)
          return (
            <DeadLetterDetail
              id={selectedId}
              onReplay={(result) => {
                setLastReplay({ job_key: current?.job_key ?? '', ...result })
              }}
              onClose={() => setSelectedId(null)}
            />
          )
        })()}
      </Sheet>
    </div>
  )
}
