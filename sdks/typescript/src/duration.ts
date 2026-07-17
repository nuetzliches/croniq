// Humane duration strings (e.g. "5m", "30s", "1h") emitted by the Croniq
// server in `WorkAssignment.timeout`. The .NET SDK supports `s/m/h/d`; we
// match that vocabulary here so the same server output parses identically.

export function parseTimeoutMs(raw: string | undefined | null): number | undefined {
  if (raw == null) return undefined;
  const text = String(raw).trim().toLowerCase();
  if (text.length < 2) return undefined;
  const unit = text[text.length - 1];
  const valueStr = text.slice(0, -1);
  const value = Number(valueStr);
  if (!Number.isFinite(value)) return undefined;
  switch (unit) {
    case 's': return value * 1_000;
    case 'm': return value * 60_000;
    case 'h': return value * 3_600_000;
    case 'd': return value * 86_400_000;
    default: return undefined;
  }
}

/**
 * Parse the server's `scheduled_for` (RFC 3339) into a Date. Returns null when
 * the field is absent (older server) or unparseable — never falls back to the
 * queue fire time, which would reintroduce the wrong-logical-time bug.
 */
export function parseScheduledFor(raw: string | undefined | null): Date | null {
  if (!raw) return null;
  const d = new Date(raw);
  return Number.isNaN(d.getTime()) ? null : d;
}
