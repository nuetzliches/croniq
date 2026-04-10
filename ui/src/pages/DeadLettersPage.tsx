import { useDeadLetters, useDeleteDeadLetter, useDeadLetter } from '@/api/hooks'
import { useState } from 'react'
import { truncate } from '@/lib/utils'

export function DeadLettersPage() {
  const deadLetters = useDeadLetters()
  const deleteDL = useDeleteDeadLetter()
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const detail = useDeadLetter(selectedId || '')

  return (
    <div className="space-y-4">
      <h1 className="text-lg font-semibold">Dead Letters</h1>

      <div className="flex gap-4">
        <div className="flex-1 bg-card border border-border rounded-lg overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-muted">
              <tr>
                <th className="text-left px-3 py-2 font-medium">Job</th>
                <th className="text-left px-3 py-2 font-medium">Attempt</th>
                <th className="text-left px-3 py-2 font-medium">Error</th>
                <th className="text-left px-3 py-2 font-medium">Created</th>
                <th className="px-3 py-2"></th>
              </tr>
            </thead>
            <tbody>
              {deadLetters.data?.map((dl) => (
                <tr key={dl.id} className={`border-t border-border cursor-pointer hover:bg-muted ${selectedId === dl.id ? 'bg-muted' : ''}`} onClick={() => setSelectedId(dl.id)}>
                  <td className="px-3 py-2 font-mono text-xs">{dl.job_key}</td>
                  <td className="px-3 py-2">{dl.attempt}</td>
                  <td className="px-3 py-2 text-muted-foreground">{truncate(dl.error, 40)}</td>
                  <td className="px-3 py-2 text-muted-foreground">{new Date(dl.created_at).toLocaleString()}</td>
                  <td className="px-3 py-2 text-right">
                    <button onClick={(e) => { e.stopPropagation(); deleteDL.mutate(dl.id) }} className="text-xs text-destructive hover:underline">Delete</button>
                  </td>
                </tr>
              ))}
              {!deadLetters.data?.length && <tr><td colSpan={5} className="px-3 py-4 text-center text-muted-foreground">No dead letters</td></tr>}
            </tbody>
          </table>
        </div>

        {selectedId && detail.data && (
          <div className="w-96 bg-card border border-border rounded-lg p-4 space-y-3">
            <h2 className="font-medium text-sm">Dead Letter Detail</h2>
            <div className="text-xs space-y-1">
              <p><span className="text-muted-foreground">ID:</span> {detail.data.id}</p>
              <p><span className="text-muted-foreground">Execution:</span> {detail.data.execution_id}</p>
              <p><span className="text-muted-foreground">Job:</span> {detail.data.job_key}</p>
              <p><span className="text-muted-foreground">Attempt:</span> {detail.data.attempt}</p>
              <p><span className="text-muted-foreground">Reason:</span> {detail.data.dead_reason}</p>
            </div>
            <div>
              <p className="text-xs text-muted-foreground mb-1">Error:</p>
              <pre className="text-xs bg-muted p-2 rounded overflow-auto max-h-48">{detail.data.error}</pre>
            </div>
            {Object.keys(detail.data.metadata).length > 0 && (
              <div>
                <p className="text-xs text-muted-foreground mb-1">Metadata:</p>
                <pre className="text-xs bg-muted p-2 rounded">{JSON.stringify(detail.data.metadata, null, 2)}</pre>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  )
}
