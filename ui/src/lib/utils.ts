import { type ClassValue, clsx } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

/// ISO 8601 in UTC: "2026-04-25 17:14:54Z". Locale-stable so screenshots
/// and bug reports read the same regardless of the user's region. Falls
/// back to `toLocaleString` if the input isn't a parseable timestamp.
export function formatDate(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  // YYYY-MM-DD HH:MM:SS in UTC. Cheaper than Intl.DateTimeFormat and
  // the most useful format when copying out of the dashboard.
  const pad = (n: number) => String(n).padStart(2, "0")
  return (
    `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())} ` +
    `${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:${pad(d.getUTCSeconds())}Z`
  )
}

/// Compact relative time: "just now", "5s ago", "3m ago", "2h ago",
/// "yesterday", "3d ago", "2026-03-12" for older. Pair with `formatDate`
/// in a tooltip so users can see both.
export function formatRelative(iso: string, now = Date.now()): string {
  const t = new Date(iso).getTime()
  if (Number.isNaN(t)) return iso
  const diffSec = Math.round((now - t) / 1000)
  if (diffSec < 0) {
    // future
    const ahead = -diffSec
    if (ahead < 60) return `in ${ahead}s`
    if (ahead < 3600) return `in ${Math.floor(ahead / 60)}m`
    if (ahead < 86400) return `in ${Math.floor(ahead / 3600)}h`
    return `in ${Math.floor(ahead / 86400)}d`
  }
  if (diffSec < 5) return "just now"
  if (diffSec < 60) return `${diffSec}s ago`
  if (diffSec < 3600) return `${Math.floor(diffSec / 60)}m ago`
  if (diffSec < 86400) return `${Math.floor(diffSec / 3600)}h ago`
  if (diffSec < 86400 * 2) return "yesterday"
  if (diffSec < 86400 * 30) return `${Math.floor(diffSec / 86400)}d ago`
  // Older: short ISO date, no time.
  return formatDate(iso).slice(0, 10)
}

export function truncate(s: string, n: number): string {
  return s.length > n ? s.slice(0, n) + "..." : s
}

/// Shorten a UUID-ish ID to its first 8 chars. Pair with the full string
/// on hover so users can copy it.
export function shortId(id: string): string {
  return id.slice(0, 8)
}
