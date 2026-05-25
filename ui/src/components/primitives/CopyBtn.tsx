import { useState } from 'react'
import { Check, Copy } from 'lucide-react'
import clsx from 'clsx'

export interface CopyBtnProps {
  value: string
  label?: string
  size?: 'sm' | 'md'
  className?: string
}

export function CopyBtn({ value, label, size = 'sm', className }: CopyBtnProps) {
  const [done, setDone] = useState(false)
  return (
    <button
      type="button"
      className={clsx('btn', 'ghost', size === 'sm' && 'sm', className)}
      onClick={() => {
        navigator.clipboard?.writeText(value).then(
          () => {
            setDone(true)
            window.setTimeout(() => setDone(false), 1200)
          },
          () => {
            /* clipboard denied — leave the button as-is */
          },
        )
      }}
      title="Copy"
      aria-label={label ? `Copy ${label}` : 'Copy'}
    >
      {done ? <Check size={13} style={{ color: 'var(--success)' }} /> : <Copy size={13} />}
      {label}
    </button>
  )
}
