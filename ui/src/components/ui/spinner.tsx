import { BrandMark } from '@/components/primitives/BrandMark'
import { cn } from '@/lib/utils'

/**
 * Brand-aware loading indicator. The orbit mark spins at 0.9 s/turn
 * inheriting the current text color, so it slots into buttons, page
 * fallbacks and the dashboard tiles without restyling.
 */
export function Spinner({ className }: { className?: string }) {
  return (
    <span
      role="status"
      aria-label="Loading"
      className={cn('inline-flex items-center justify-center text-muted-foreground', className)}
    >
      <BrandMark spinning size="1em" />
    </span>
  )
}
