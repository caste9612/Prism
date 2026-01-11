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

  // Helper to update and persist settings
  const updateAndSave = (updater: (settings: Settings) => Settings) => {
    update(settings => {
      const newSettings = updater(settings);
      saveSettings(newSettings);
      return newSettings;
    });
  };

  return {
    subscribe,

    updateSetting: <K extends keyof Settings>(key: K, value: Settings[K]) => {
      updateAndSave(settings => ({ ...settings, [key]: value }));
    },

    setAll: (newSettings: Partial<Settings>) => {
      updateAndSave(settings => ({ ...settings, ...newSettings }));
    },

    addExcludePattern: (pattern: string) => {
      updateAndSave(settings => {
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
      updateAndSave(settings => ({
        ...settings,
        excludePatterns: settings.excludePatterns.filter(p => p !== pattern),
      }));
    },

    reset: () => {
      set(defaultSettings);
      saveSettings(defaultSettings);
    },
  };
}

export const settingsStore = createSettingsStore();
