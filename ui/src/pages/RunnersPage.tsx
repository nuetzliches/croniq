import { useRunners, useDeleteRunner } from '@/api/hooks'

const statusColors: Record<string, string> = {
  Online: 'bg-green-100 text-green-700',
  Stale: 'bg-yellow-100 text-yellow-700',
  Dead: 'bg-red-100 text-red-700',
}

export function RunnersPage() {
  const runners = useRunners()
  const deleteRunner = useDeleteRunner()

  return (
    <div className="space-y-4">
      <h1 className="text-lg font-semibold">Runners</h1>
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
            {runners.data?.map((r) => (
              <tr key={r.runner_id} className="border-t border-border">
                <td className="px-3 py-2 font-mono text-xs">{r.runner_id}</td>
                <td className="px-3 py-2">
                  <span className={`px-2 py-0.5 rounded text-xs font-medium ${statusColors[r.status] || 'bg-gray-100 text-gray-700'}`}>{r.status}</span>
                </td>
                <td className="px-3 py-2">{r.capabilities.length ? r.capabilities.join(', ') : '-'}</td>
                <td className="px-3 py-2">{r.inflight} / {r.max_inflight}</td>
                <td className="px-3 py-2 text-muted-foreground">{new Date(r.last_poll_at).toLocaleString()}</td>
                <td className="px-3 py-2 text-right">
                  <button onClick={() => deleteRunner.mutate(r.runner_id)} className="text-xs text-destructive hover:underline">Remove</button>
                </td>
              </tr>
            ))}
            {!runners.data?.length && <tr><td colSpan={6} className="px-3 py-4 text-center text-muted-foreground">No runners connected</td></tr>}
          </tbody>
        </table>
      </div>
    </div>
  )
}
