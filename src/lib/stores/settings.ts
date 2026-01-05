import { writable } from 'svelte/store';
import { browser } from '$app/environment';
import type { Settings } from '$lib/types';

const defaultSettings: Settings = {
  excludeHidden: true,
  excludeSystem: true,
  minFileSize: 0,
  excludePatterns: [
    'node_modules',
    '.git',
    '$RECYCLE.BIN',
    'System Volume Information',
  ],
  theme: 'dark',
};

const STORAGE_KEY = 'prism-settings';

function loadSettings(): Settings {
  if (!browser) return defaultSettings;

  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored) {
      const parsed = JSON.parse(stored);
      return { ...defaultSettings, ...parsed };
    }
  } catch (e) {
    console.error('Failed to load settings:', e);
  }

  return defaultSettings;
}

function saveSettings(settings: Settings): void {
  if (!browser) return;

  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  } catch (e) {
    console.error('Failed to save settings:', e);
  }
}

function createSettingsStore() {
  const { subscribe, set, update } = writable<Settings>(loadSettings());

  // Save to localStorage whenever settings change
  subscribe(settings => {
    saveSettings(settings);
  });

  return {
    subscribe,

    updateSetting: <K extends keyof Settings>(key: K, value: Settings[K]) => {
      update(settings => ({ ...settings, [key]: value }));
    },

    setAll: (newSettings: Partial<Settings>) => {
      update(settings => ({ ...settings, ...newSettings }));
    },

    addExcludePattern: (pattern: string) => {
      update(settings => {
        if (!settings.excludePatterns.includes(pattern)) {
          return {
            ...settings,
            excludePatterns: [...settings.excludePatterns, pattern],
          };
        }
        return settings;
      });
    },

    removeExcludePattern: (pattern: string) => {
      update(settings => ({
        ...settings,
        excludePatterns: settings.excludePatterns.filter(p => p !== pattern),
      }));
    },

    reset: () => {
      set(defaultSettings);
    },
  };
}

export const settingsStore = createSettingsStore();
