import { forwardRef } from 'react'
import { useCalendars } from '@/api/hooks'

interface Props extends Omit<React.SelectHTMLAttributes<HTMLSelectElement>, 'children'> {
  /// Wrapper class — applied to the surrounding `<div>` so the field can
  /// be inlined in grid rows next to other controls.
  wrapperClassName?: string
}

/// Calendar dropdown for gating schedule execution. Backed by
/// `useCalendars()` — the value sent to the API is the calendar **name**
/// (the runtime resolves names to compiled calendars; see
/// `crates/croniq-server/src/loader.rs` job_cfg.calendar handling).
///
/// Empty value (`""`) means "no calendar gate". The dropdown always
/// includes a "(none)" entry so users can clear a previously-selected
/// calendar without typing.
export const CalendarPicker = forwardRef<HTMLSelectElement, Props>(
  function CalendarPicker({ wrapperClassName, className, ...rest }, ref) {
    const calendars = useCalendars()
    return (
      <div className={wrapperClassName}>
        <label className="block text-xs text-muted-foreground mb-1">
          Calendar (optional)
        </label>
        <select
          ref={ref}
          className={`w-full px-3 py-2 border border-border rounded-md text-sm bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary ${className ?? ''}`}
          {...rest}
        >
          <option value="">— No calendar —</option>
          {calendars.data?.map((c) => (
            <option key={c.calendar_id} value={c.name}>
              {c.name}{c.managed_by === 'dsl' ? ' (DSL)' : ''}
            </option>
          ))}
        </select>
        <p className="text-[11px] text-muted-foreground mt-1">
          Restricts when the schedule may fire (e.g. business hours, holidays).
        </p>
      </div>
    )
  },
)
