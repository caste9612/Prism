// Translations (ADR-0014): no visible text is written directly in the markup; every string
// comes from a locale file. English is the fallback for missing keys.
import en from './locales/en.json';
import it from './locales/it.json';

export type Lang = 'en' | 'it';
export type LangPreference = 'system' | Lang;
type Dict = { [key: string]: string | Dict };

export const dictionaries: Record<Lang, Dict> = { en, it };
export const languages: Lang[] = ['en', 'it'];

/** Windows language if supported, English otherwise (SPEC §4). */
export function systemLanguage(): Lang {
  const nav = typeof navigator === 'undefined' ? 'en' : navigator.language;
  return nav.toLowerCase().startsWith('it') ? 'it' : 'en';
}

let current = $state<Lang>(systemLanguage());

export function setLanguage(pref: LangPreference): void {
  current = pref === 'system' ? systemLanguage() : pref;
  if (typeof document !== 'undefined') document.documentElement.lang = current;
}

export function language(): Lang {
  return current;
}

function lookup(dict: Dict, key: string): string | undefined {
  let node: string | Dict | undefined = dict;
  for (const part of key.split('.')) {
    if (typeof node !== 'object' || node === null) return undefined;
    node = node[part];
  }
  return typeof node === 'string' ? node : undefined;
}

/** Replaces `{name}` placeholders. */
export function interpolate(text: string, params?: Record<string, string | number>): string {
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (m, name: string) =>
    name in params ? String(params[name]) : m,
  );
}

/**
 * The translation of `key` in the current language. With `count` equal to 1 the singular form
 * `key_one` is used when the dictionary has it ("1 hour ago", not "1 hours ago").
 */
export function t(key: string, params?: Record<string, string | number>): string {
  const one = params?.count === 1 || params?.count === '1';
  const dict = dictionaries[current];
  const text =
    (one ? (lookup(dict, `${key}_one`) ?? lookup(en, `${key}_one`)) : undefined) ??
    lookup(dict, key) ??
    lookup(en, key) ??
    key;
  return interpolate(text, params);
}

/** Every key path of a dictionary (for tests). */
export function keys(dict: Dict, prefix = ''): string[] {
  return Object.entries(dict).flatMap(([k, v]) =>
    typeof v === 'string' ? [prefix + k] : keys(v, `${prefix}${k}.`),
  );
}
