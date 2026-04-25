import { Trash2, Wifi, WifiOff } from 'lucide-react'
import { useRunnersSSE, useDeleteRunner } from '@/api/hooks'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/ui/empty-state'
import { CopyButton } from '@/components/ui/copy-button'
import { RelativeTime } from '@/components/ui/relative-time'
import { useConfirm } from '@/components/ui/confirm-dialog'

function CapacityRing({ inflight, max }: { inflight: number; max: number }) {
  const pct = max > 0 ? Math.min(inflight / max, 1) : 0
  const r = 20, cx = 26, cy = 26, circ = 2 * Math.PI * r
  const fill = circ * pct
  const color = pct < 0.6 ? 'var(--color-status-ok-fg)' : pct < 0.9 ? 'var(--color-status-warn-fg)' : 'var(--color-status-err-fg)'
  return (
    <svg width="52" height="52" aria-label={`${inflight} of ${max} inflight`} role="img">
      <circle cx={cx} cy={cy} r={r} fill="none" stroke="var(--color-border)" strokeWidth="4" />
      {max > 0 && (
        <circle cx={cx} cy={cy} r={r} fill="none" stroke={color} strokeWidth="4"
          strokeDasharray={`${fill} ${circ}`} strokeLinecap="round"
          transform={`rotate(-90 ${cx} ${cy})`} />
      )}
      <text x={cx} y={cy} textAnchor="middle" dominantBaseline="middle"
        fontSize="9" fill="currentColor" fontWeight="600">
        {inflight}/{max}
      </text>
    </svg>
  )
}

const statusVariant = (s: string) =>
  s === 'Online' ? 'ok' : s === 'Stale' ? 'warn' : 'err'

export function RunnersPage() {
  const { data: runners, isConnected } = useRunnersSSE()
  const deleteRunner = useDeleteRunner()
  const { confirm, dialog: confirmDialog } = useConfirm()

  async function handleDelete(runnerId: string) {
    const ok = await confirm({
      title: `Remove runner ${runnerId}?`,
      description:
        'In-flight executions belonging to this runner stay claimed until their lease expires, then time out. Use the runner shutdown signal for a graceful drain.',
      confirmLabel: 'Remove runner',
      destructive: true,
    })
    if (ok) deleteRunner.mutate(runnerId)
  }

  return (
    <div className="space-y-4">
      {confirmDialog}
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">{runners?.length ?? 0} runners</p>
        <span
          role="status"
          aria-live="polite"
          className="flex items-center gap-1.5 text-xs text-muted-foreground"
        >
          {isConnected
            ? <><Wifi className="h-3.5 w-3.5 text-status-ok-fg" /><span className="text-status-ok-fg">Live</span></>
            : <><WifiOff className="h-3.5 w-3.5" />Reconnecting…</>}
        </span>
      </div>

      {!runners?.length && (
        <EmptyState
          icon={<Wifi className="h-10 w-10" />}
          title="No runners connected"
          description="Start a runner with the Runner SDK to see it here"
        />
      )}

      <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
        {runners?.map((r) => (
          <Card key={r.runner_id}>
            <CardContent className="pt-4">
              <div className="flex items-start justify-between gap-3">
                <div className="flex-1 min-w-0 space-y-2">
                  <div className="flex items-center gap-2">
                    <Badge variant={statusVariant(r.status)}>{r.status}</Badge>
                    <span className="font-mono text-xs text-muted-foreground truncate" title={r.runner_id}>{r.runner_id}</span>
                    <CopyButton value={r.runner_id} label={`Copy runner id ${r.runner_id}`} />
                  </div>

                  {r.capabilities.length > 0 && (
                    <div className="flex flex-wrap gap-1">
                      {r.capabilities.map((c) => (
                        <span key={c} className="inline-flex items-center rounded-full bg-accent px-2 py-0.5 text-xs text-accent-foreground">
                          {c}
                        </span>
                      ))}
                    </div>
                  )}

                  <p className="text-xs text-muted-foreground">
                    Last poll <RelativeTime iso={r.last_poll_at} />
                  </p>
                </div>

                <div className="flex flex-col items-center gap-2 shrink-0">
                  <CapacityRing inflight={r.inflight} max={r.max_inflight} />
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => handleDelete(r.runner_id)}
                    aria-label={`Remove runner ${r.runner_id}`}
                    className="h-6 w-6 p-0 text-muted-foreground hover:text-destructive"
                  >
                    <Trash2 className="h-3 w-3" />
                  </Button>
                </div>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>
    </div>
  )
}
