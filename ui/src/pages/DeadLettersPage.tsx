import { Fragment, useState } from 'react'
import { TriangleAlert, RotateCcw, Trash2, MailX } from 'lucide-react'
import { useDeadLetters, useDeleteDeadLetter, useDeadLetter, useReplayDeadLetter } from '@/api/hooks'
import { Sheet } from '@/components/ui/sheet'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/ui/empty-state'
import { Spinner } from '@/components/ui/spinner'
import { truncate } from '@/lib/utils'

function DeadLetterDetail({ id }: { id: string }) {
  const { data, isLoading } = useDeadLetter(id)
  const replay = useReplayDeadLetter()
  const del = useDeleteDeadLetter()

  if (isLoading) return <div className="flex justify-center py-8"><Spinner className="h-5 w-5" /></div>
  if (!data) return null

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-x-4 gap-y-1.5 text-xs">
        {[
          ['Job', data.job_key],
          ['Attempt', data.attempt],
          ['Reason', data.dead_reason],
          ['Execution ID', data.execution_id.slice(0, 16) + '…'],
          ['Created', new Date(data.created_at).toLocaleString()],
          ['Expires', data.expires_at ? new Date(data.expires_at).toLocaleString() : '—'],
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
          onClick={() => replay.mutate(data.id)}
          disabled={replay.isPending}
        >
          {replay.isPending ? <Spinner className="h-3.5 w-3.5" /> : <RotateCcw className="h-3.5 w-3.5" />}
          Replay
        </Button>
        <Button
          size="sm"
          variant="destructive"
          onClick={() => del.mutate(data.id)}
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
  const [selectedId, setSelectedId] = useState<string | null>(null)

  return (
    <div className="space-y-4">
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
                  <td className="px-3 py-2.5 text-muted-foreground">{new Date(dl.created_at).toLocaleString()}</td>
                  <td className="px-3 py-2.5">
                    <div className="flex items-center justify-end gap-1" onClick={(e) => e.stopPropagation()}>
                      <Button
                        variant="ghost" size="sm"
                        onClick={() => replay.mutate(dl.id)}
                        disabled={replay.isPending}
                        aria-label="Replay"
                        className="h-7 w-7 p-0"
                      >
                        <RotateCcw className="h-3.5 w-3.5" />
                      </Button>
                      <Button
                        variant="ghost" size="sm"
                        onClick={() => deleteDL.mutate(dl.id)}
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
        {selectedId && <DeadLetterDetail id={selectedId} />}
      </Sheet>
    </div>
  )
}
