import { useState } from 'react'
import { Check, Copy } from 'lucide-react'
import { cn } from '@/lib/utils'

interface CopyButtonProps {
  value: string
  label?: string
  className?: string
  size?: 'sm' | 'md'
}

/// One-shot "copy to clipboard" button with a 2-second checkmark
/// confirmation. The accessible label includes a snippet of the value so
/// screen-reader users know what they're about to copy.
export function CopyButton({ value, label, className, size = 'sm' }: CopyButtonProps) {
  const [copied, setCopied] = useState(false)
  const iconSize = size === 'sm' ? 'h-3.5 w-3.5' : 'h-4 w-4'

  function copy(e: React.MouseEvent) {
    e.stopPropagation()
    navigator.clipboard
      .writeText(value)
      .then(() => {
        setCopied(true)
        setTimeout(() => setCopied(false), 2000)
      })
      .catch(() => {
        /* clipboard API can fail in some test harnesses; ignore */
      })
  }

  return (
    <button
      onClick={copy}
      aria-label={label ?? `Copy ${value.slice(0, 16)}${value.length > 16 ? '…' : ''}`}
      className={cn(
        'inline-flex items-center justify-center rounded-sm text-muted-foreground hover:text-foreground transition-colors',
        className
      )}
    >
      {copied ? (
        <Check className={cn(iconSize, 'text-status-ok-fg')} />
      ) : (
        <Copy className={iconSize} />
      )}
    </button>
  )
}
