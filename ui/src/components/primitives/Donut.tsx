export interface DonutProps {
  value: number
  max: number
  size?: number
  thickness?: number
  label?: string
  /** Override the auto-tier color (mute → success → warn → error). */
  color?: string
}

export function Donut({ value, max, size = 36, thickness = 3, label, color }: DonutProps) {
  const r = size / 2 - thickness
  const c = 2 * Math.PI * r
  const pct = max === 0 ? 0 : Math.max(0, Math.min(1, value / max))
  const offset = c * (1 - pct)
  const tone =
    color ??
    (pct === 0
      ? 'var(--fg-mute)'
      : pct < 0.5
        ? 'var(--success)'
        : pct < 0.9
          ? 'var(--warn)'
          : 'var(--error)')
  return (
    <div style={{ position: 'relative', width: size, height: size, flexShrink: 0 }}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="var(--bg-3)" strokeWidth={thickness} />
        <circle
          cx={size / 2}
          cy={size / 2}
          r={r}
          fill="none"
          stroke={tone}
          strokeWidth={thickness}
          strokeDasharray={c}
          strokeDashoffset={offset}
          strokeLinecap="round"
          transform={`rotate(-90 ${size / 2} ${size / 2})`}
          style={{ transition: 'stroke-dashoffset .5s var(--easing)' }}
        />
      </svg>
      <div
        style={{
          position: 'absolute',
          inset: 0,
          display: 'grid',
          placeItems: 'center',
          fontFamily: 'var(--font-mono-app)',
          fontSize: 10,
          color: 'var(--fg-2)',
        }}
      >
        {label ?? `${value}/${max}`}
      </div>
    </div>
  )
}
