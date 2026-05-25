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
