import clsx from 'clsx'

export interface BrandMarkProps {
  size?: number | string
  className?: string
  /** Spin the mark — used as the loading indicator on async actions. */
  spinning?: boolean
  /** Add the brand purple chip background (favicon look). Off renders the
   *  glyph alone in currentColor, suitable for inline use inside coloured
   *  containers like `.gear` or `.login-mark`. */
  chip?: boolean
  title?: string
}

/**
 * The Croniq orbit mark, ported from icons/mark-mono.svg. Renders inline
 * so it inherits text color and avoids a network roundtrip for the
 * `<img src>` form. Pair with the global `.brand-spin` rule to spin it
 * at 0.9 s/turn — see ui/src/styles/components.css.
 */
export function BrandMark({ size = 18, className, spinning = false, chip = false, title }: BrandMarkProps) {
  const role = title ? 'img' : undefined
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 100 100"
      xmlns="http://www.w3.org/2000/svg"
      className={clsx(spinning && 'brand-spin', className)}
      role={role}
      aria-label={title}
      aria-hidden={title ? undefined : true}
    >
      <defs>
        <mask id="croniq-mark-gap">
          <rect width="100" height="100" fill="white" />
          <circle cx="76" cy="76" r="12" fill="black" />
        </mask>
      </defs>
      {chip ? <rect width="100" height="100" rx="20" fill="#6A54DF" /> : null}
      <circle
        cx="50"
        cy="50"
        r="34"
        fill="none"
        stroke={chip ? '#ffffff' : 'currentColor'}
        strokeWidth="8"
        mask="url(#croniq-mark-gap)"
      />
      <circle cx="76" cy="76" r="9" fill={chip ? '#ffffff' : 'currentColor'} />
    </svg>
  )
}
