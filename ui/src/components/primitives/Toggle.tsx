export interface ToggleProps {
  on: boolean
  onChange: (next: boolean) => void
  label?: string
  disabled?: boolean
}

export function Toggle({ on, onChange, label, disabled = false }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!on)}
      style={{
        width: 32,
        height: 18,
        borderRadius: 999,
        background: on ? 'var(--accent)' : 'var(--bg-3)',
        border: `1px solid ${on ? 'var(--accent)' : 'var(--border-2)'}`,
        position: 'relative',
        transition: 'background .15s var(--easing), border-color .15s var(--easing)',
        cursor: disabled ? 'not-allowed' : 'pointer',
        opacity: disabled ? 0.55 : 1,
        padding: 0,
      }}
    >
      <span
        style={{
          position: 'absolute',
          top: 1,
          left: on ? 15 : 1,
          width: 14,
          height: 14,
          borderRadius: 999,
          background: 'white',
          transition: 'left .15s var(--easing)',
          boxShadow: '0 1px 2px rgba(0,0,0,.2)',
        }}
      />
    </button>
  )
}
