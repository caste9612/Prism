// Presentation logic of the search (F07), kept out of the components to be testable.

/**
 * Sets the filter `key` in the search text to `value` (replacing any `key:...` already
 * there), or removes it when `value` is null: the controls of the search bar are equivalent to
 * typing the filter (F07-AC5).
 */
export function setFilter(text: string, key: string, value: string | null): string {
  const pattern = new RegExp(`(^|\\s)${key}:("[^"]*"|\\S*)`, 'gi');
  const without = text.replace(pattern, ' ').replace(/\s+/g, ' ').trim();
  if (value === null || value === '') return without;
  const quoted = /\s/.test(value) ? `"${value}"` : value;
  return `${without} ${key}:${quoted}`.trim();
}

/** The value of filter `key` in the text, if present (without quotes). */
export function getFilter(text: string, key: string): string | null {
  const m = new RegExp(`(?:^|\\s)${key}:("[^"]*"|\\S*)`, 'i').exec(text);
  return m ? m[1]!.replace(/^"|"$/g, '') : null;
}

/** Splits the text around the error range, to show where the mistake is (F07-AC6). */
export function splitAt(text: string, start: number, end: number): [string, string, string] {
  const chars = [...text];
  const s = Math.max(0, Math.min(start, chars.length));
  const e = Math.max(s, Math.min(end, chars.length));
  return [chars.slice(0, s).join(''), chars.slice(s, e).join(''), chars.slice(e).join('')];
}

/** The user's offset from UTC in minutes (positive east of Greenwich), for typed dates. */
export function utcOffsetMinutes(date = new Date()): number {
  return -date.getTimezoneOffset();
}
