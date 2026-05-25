import { useId, useMemo } from 'react'
import clsx from 'clsx'

export interface SparklineProps {
  data: number[]
  height?: number
  width?: number
  color?: string
  fill?: boolean
  className?: string
}

export function Sparkline({
  data,
  height = 36,
  width = 200,
  color = 'var(--accent)',
  fill = true,
  className,
}: SparklineProps) {
  const gradId = useId().replace(/:/g, '_')

  const { dPath, dFill } = useMemo(() => {
    if (data.length < 2) {
      return { dPath: '', dFill: '' }
    }
    const max = Math.max(...data, 1)
    const min = Math.min(...data, 0)
    const range = Math.max(max - min, 1)
    const step = width / (data.length - 1)
    const pts = data.map((v, i): [number, number] => [
      i * step,
      height - 4 - ((v - min) / range) * (height - 8),
    ])
    const d = pts
      .map(([x, y], i) => (i === 0 ? 'M' : 'L') + x.toFixed(1) + ',' + y.toFixed(1))
      .join(' ')
    const f = d + ` L${width},${height} L0,${height} Z`
    return { dPath: d, dFill: f }
  }, [data, width, height])

  if (!dPath) {
    return <svg className={clsx('spark', className)} aria-hidden />
  }

  return (
    <svg
      className={clsx('spark', className)}
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      aria-hidden
    >
      {fill ? (
        <defs>
          <linearGradient id={gradId} x1={0} y1={0} x2={0} y2={1}>
            <stop offset="0%" stopColor={color} stopOpacity={0.35} />
            <stop offset="100%" stopColor={color} stopOpacity={0} />
          </linearGradient>
        </defs>
      ) : null}
      {fill ? <path d={dFill} fill={`url(#${gradId})`} /> : null}
      <path
        d={dPath}
        fill="none"
        stroke={color}
        strokeWidth={1.5}
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  )
}
