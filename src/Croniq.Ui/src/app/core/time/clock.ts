export function nowMs(): number {
  return Date.now();
}

export function epochMsFromIso(value: string): number | null {
  if (!value) {
    return null;
  }
  const timestamp = Date.parse(value);
  return Number.isFinite(timestamp) ? timestamp : null;
}

export function isoFromEpochMs(epochMs: number): string {
  return new Date(epochMs).toISOString();
}

export function utcHourFromEpochMs(epochMs: number): number {
  return new Date(epochMs).getUTCHours();
}

export function tryIsoFromUnknown(value: unknown): string | null {
  if (typeof value === 'string') {
    const timestamp = epochMsFromIso(value.trim());
    return timestamp != null ? isoFromEpochMs(timestamp) : null;
  }
  if (typeof value === 'number') {
    return Number.isFinite(value) ? isoFromEpochMs(value) : null;
  }
  if (value instanceof Date) {
    const timestamp = value.getTime();
    return Number.isFinite(timestamp) ? isoFromEpochMs(timestamp) : null;
  }
  return null;
}

export function nowIso(): string {
  return isoFromEpochMs(nowMs());
}
