import { useRunnersSSE, useDeleteRunner } from '@/api/hooks'

const statusColors: Record<string, string> = {
  Online: 'bg-status-ok-bg text-status-ok-fg',
  Stale: 'bg-status-warn-bg text-status-warn-fg',
  Dead: 'bg-status-err-bg text-status-err-fg',
}

export function RunnersPage() {
  const { data: runnersData, isConnected } = useRunnersSSE()
  const deleteRunner = useDeleteRunner()

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-semibold">Runners</h1>
        <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <span className={`w-2 h-2 rounded-full ${isConnected ? 'bg-status-ok-fg' : 'bg-status-neutral-fg'}`} />
          {isConnected ? 'Live' : 'Reconnecting…'}
        </span>
      </div>
      <div className="bg-card border border-border rounded-lg overflow-hidden">
        <table className="w-full text-sm">
          <thead className="bg-muted">
            <tr>
              <th className="text-left px-3 py-2 font-medium">Runner ID</th>
              <th className="text-left px-3 py-2 font-medium">Status</th>
              <th className="text-left px-3 py-2 font-medium">Capabilities</th>
              <th className="text-left px-3 py-2 font-medium">Inflight</th>
              <th className="text-left px-3 py-2 font-medium">Last Poll</th>
              <th className="px-3 py-2"></th>
            </tr>
          </thead>
          <tbody>
            {runnersData?.map((r) => (
              <tr key={r.runner_id} className="border-t border-border">
                <td className="px-3 py-2 font-mono text-xs">{r.runner_id}</td>
                <td className="px-3 py-2">
                  <span className={`px-2 py-0.5 rounded text-xs font-medium ${statusColors[r.status] || 'bg-status-neutral-bg text-status-neutral-fg'}`}>{r.status}</span>
                </td>
                <td className="px-3 py-2">{r.capabilities.length ? r.capabilities.join(', ') : '-'}</td>
                <td className="px-3 py-2">{r.inflight} / {r.max_inflight}</td>
                <td className="px-3 py-2 text-muted-foreground">{new Date(r.last_poll_at).toLocaleString()}</td>
                <td className="px-3 py-2 text-right">
                  <button onClick={() => deleteRunner.mutate(r.runner_id)} className="text-xs text-destructive hover:underline">Remove</button>
                </td>
              </tr>
            ))}
            {!runnersData?.length && <tr><td colSpan={6} className="px-3 py-4 text-center text-muted-foreground">No runners connected</td></tr>}
          </tbody>
        </table>
      </div>
    </div>
  )
}
