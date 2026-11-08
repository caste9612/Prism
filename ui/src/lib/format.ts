// Formatting of sizes, counts and dates in the interface language.

const UNITS = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];

/** 1536 → "1.5 KB" (decimal units, as Windows Explorer shows disk sizes). */
export function formatBytes(bytes: number, locale = 'en'): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—';
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < UNITS.length - 1) {
    value /= 1000;
    unit++;
  }
  const digits = unit === 0 || value >= 100 ? 0 : 1;
  return `${value.toLocaleString(locale, { maximumFractionDigits: digits, minimumFractionDigits: digits })} ${UNITS[unit]}`;
}

/** 1234567 → "1,234,567" (or "1.234.567" in Italian). */
export function formatCount(n: number, locale = 'en'): string {
  return n.toLocaleString(locale);
}

export type Ago =
  | { key: 'time.justNow' }
  | { key: 'time.minutes' | 'time.hours' | 'time.days'; count: number };

/** How long ago `then` (Unix ms) was, as a translation key and count. */
export function ago(then: number, now: number = Date.now()): Ago {
  const minutes = Math.floor(Math.max(0, now - then) / 60_000);
  if (minutes < 1) return { key: 'time.justNow' };
  if (minutes < 60) return { key: 'time.minutes', count: minutes };
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return { key: 'time.hours', count: hours };
  return { key: 'time.days', count: Math.floor(hours / 24) };
}

/** Used fraction of a volume, between 0 and 1, or null when unknown. */
export function usedFraction(capacity: number | null, free: number | null): number | null {
  if (capacity === null || free === null || capacity <= 0) return null;
  return Math.min(1, Math.max(0, (capacity - free) / capacity));
}

/** A length of time, short: "40 s", "12 min", "1 h 05 min" (the same in both languages). */
export function formatDuration(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s} s`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  return `${h} h ${String(m % 60).padStart(2, '0')} min`;
}
