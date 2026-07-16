import type { MouseEvent, ReactNode } from 'react'
import { Link } from 'react-router'
import clsx from 'clsx'

/**
 * Cross-links between the three core entities (jobs, executions, runners).
 *
 * These wrap react-router's <Link> and render as `.xlink` (a small
 * accent-coloured inline link defined in components.css) so they read the same
 * whether they sit in a custom-token page (`.mono` text) or a shadcn/Tailwind
 * component (`font-mono`). Pass the ambient mono class via `className` at the
 * call site to keep the surrounding look identical — only now it's clickable.
 *
 * All three encode the id: job keys legitimately contain `:` (e.g.
 * `runner:cleanup`) and must be percent-encoded to survive the route param.
 *
 * onClick stops propagation so an inline link placed inside a clickable row
 * (master-detail row selection, dashboard rows, …) navigates to its own target
 * instead of triggering the row's handler. It is harmless where there is no
 * parent handler, so every entity link stops by default.
 */

function stopRowSelect(e: MouseEvent) {
  e.stopPropagation()
}

type EntityLinkProps = {
  /** Override the visible text; defaults to the id/key itself. */
  children?: ReactNode
  /** Ambient font/utility classes for the surrounding context (e.g. `mono`). */
  className?: string
  title?: string
}

export function JobLink({
  jobKey,
  children,
  className,
  title,
}: { jobKey: string } & EntityLinkProps) {
  return (
    <Link
      to={`/jobs/${encodeURIComponent(jobKey)}`}
      className={clsx('xlink', className)}
      onClick={stopRowSelect}
      title={title ?? `Open job ${jobKey}`}
    >
      {children ?? jobKey}
    </Link>
  )
}

export function RunnerLink({
  runnerId,
  children,
  className,
  title,
}: { runnerId: string } & EntityLinkProps) {
  return (
    <Link
      to={`/runners/${encodeURIComponent(runnerId)}`}
      className={clsx('xlink', className)}
      onClick={stopRowSelect}
      title={title ?? `Open runner ${runnerId}`}
    >
      {children ?? runnerId}
    </Link>
  )
}

export function ExecutionLink({
  id,
  children,
  className,
  title,
}: { id: string } & EntityLinkProps) {
  return (
    <Link
      to={`/executions/${encodeURIComponent(id)}`}
      className={clsx('xlink', className)}
      onClick={stopRowSelect}
      title={title ?? `Open execution ${id}`}
    >
      {children ?? id}
    </Link>
  )
}
