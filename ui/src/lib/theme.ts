// Theme: follows Windows light/dark by default, with a manual choice (SPEC §4, ADR-0014).
export type ThemePreference = 'system' | 'light' | 'dark';

let preference: ThemePreference = 'system';
let media: MediaQueryList | null = null;

function apply(): void {
  const dark =
    preference === 'dark' || (preference === 'system' && (media?.matches ?? false));
  document.documentElement.dataset.theme = dark ? 'dark' : 'light';
}

export function setTheme(pref: ThemePreference): void {
  preference = pref;
  if (!media && typeof window !== 'undefined' && window.matchMedia) {
    media = window.matchMedia('(prefers-color-scheme: dark)');
    media.addEventListener('change', apply);
  }
  apply();
}
