import { memo, useMemo, useState } from 'react'
import { useExecutionLogs } from '@/api/hooks'
import { Spinner } from '@/components/ui/spinner'
import type { ExecutionLogEntry } from '@/api/types'
import { emptyLogsMessage } from '@/lib/utils'

const LEVELS = ['info', 'warn', 'error'] as const
type Level = (typeof LEVELS)[number] | 'all'

const LEVEL_COLOR: Record<string, string> = {
  error: 'text-status-err-fg',
  warn: 'text-status-warn-fg',
  info: 'text-muted-foreground',
}

const LogLine = memo(function LogLine({ level, message }: { level: string; message: string }) {
  const color = LEVEL_COLOR[level] ?? 'text-muted-foreground'
  return (
    <div className="text-xs font-mono leading-5">
      <span className={color}>[{level}]</span>{' '}
      {message}
    </div>
  )
})

/// Renders the Execution Detail Logs panel: level filter chips, search box,
/// and the (up to 10k) log line list. Per-line emission means a typical
/// shell-runner job can produce hundreds or thousands of rows here, so
/// LogLine is memo'd and filtering is done in a memoized derivation.
export function LogsPanel({
  executionId,
  executionState,
}: {
  executionId: string
  executionState?: string
}) {
  const logs = useExecutionLogs(executionId)
  const [level, setLevel] = useState<Level>('all')
  const [query, setQuery] = useState('')

  const filtered: ExecutionLogEntry[] = useMemo(() => {
    if (!logs.data) return []
    const q = query.trim().toLowerCase()
    return logs.data.filter((l) => {
      if (level !== 'all' && l.level !== level) return false
      if (q && !l.message.toLowerCase().includes(q)) return false
      return true
    })
  }, [logs.data, level, query])

  const counts = useMemo(() => {
    const c: Record<string, number> = { info: 0, warn: 0, error: 0 }
    for (const l of logs.data ?? []) {
      if (c[l.level] !== undefined) c[l.level]! += 1
    }
    return c
  }, [logs.data])

  return (
    <div>
      <div className="flex items-center justify-between mb-1.5 gap-3">
        <p className="text-xs text-muted-foreground">Logs</p>
        {(logs.data?.length ?? 0) > 0 && (
          <div className="flex items-center gap-2">
            <LevelChip
              label="all"
              active={level === 'all'}
              count={logs.data?.length ?? 0}
              onClick={() => setLevel('all')}
            />
            {LEVELS.map((l) => (
              <LevelChip
                key={l}
                label={l}
                active={level === l}
                count={counts[l] ?? 0}
                onClick={() => setLevel(l)}
              />
            ))}
            <input
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="filter…"
              className="text-xs bg-muted rounded px-2 py-0.5 border border-border w-36 focus:outline-none focus:ring-1 focus:ring-ring"
            />
          </div>
        )}
      </div>

      {logs.isLoading && <Spinner className="h-4 w-4" />}
      {logs.data?.length === 0 && (
        <p className="text-xs text-muted-foreground">{emptyLogsMessage(executionState ?? '')}</p>
      )}
      {(logs.data?.length ?? 0) > 0 && filtered.length === 0 && (
        <p className="text-xs text-muted-foreground italic">No matching log lines</p>
      )}
      {filtered.length > 0 && (
        <div className="bg-muted rounded-md p-3 max-h-64 overflow-auto space-y-0.5">
          {filtered.map((l) => (
            <LogLine key={l.id} level={l.level} message={l.message} />
          ))}
        </div>
      )}
    </div>
  )
}

function LevelChip({
  label,
  active,
  count,
  onClick,
}: {
  label: string
  active: boolean
  count: number
  onClick: () => void
}) {
  const base = 'text-xs px-2 py-0.5 rounded-full border transition-colors'
  const style = active
    ? 'bg-foreground text-background border-foreground'
    : 'bg-transparent text-muted-foreground border-border hover:bg-accent'
  return (
    <button type="button" onClick={onClick} className={`${base} ${style}`}>
      {label} <span className="font-mono opacity-70">{count}</span>
    </button>
  )
}
