// Access to the backend: Tauri commands and events, with the types generated from Rust
// (ADR-0014). Outside Tauri (browser development, tests) a simulated backend is used.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ApiError } from './bindings/ApiError';
import type { AppInfo } from './bindings/AppInfo';
import type { JobView } from './bindings/JobView';
import type { LabelsInput } from './bindings/LabelsInput';
import type { Settings } from './bindings/Settings';
import type { VolumeView } from './bindings/VolumeView';
import { t } from './i18n.svelte';
import { mockBackend } from './mock';

export type Unlisten = () => void;

export interface Backend {
  appInfo(): Promise<AppInfo>;
  volumes(): Promise<VolumeView[]>;
  jobs(): Promise<JobView[]>;
  indexVolume(id: number): Promise<number>;
  cancelJob(id: number): Promise<boolean>;
  setLabels(id: number, labels: LabelsInput): Promise<void>;
  setIgnored(id: number, ignored: boolean): Promise<void>;
  removeVolume(id: number): Promise<void>;
  getSettings(): Promise<Settings>;
  setSettings(settings: Settings): Promise<void>;
  setTrayLabels(open: string, quit: string): Promise<void>;
  onVolumesChanged(handler: () => void): Promise<Unlisten>;
  onJobsChanged(handler: (jobs: JobView[]) => void): Promise<Unlisten>;
}

const tauriBackend: Backend = {
  appInfo: () => invoke('app_info'),
  volumes: () => invoke('get_volumes'),
  jobs: () => invoke('get_jobs'),
  indexVolume: (id) => invoke('index_volume', { id }),
  cancelJob: (id) => invoke('cancel_job', { id }),
  setLabels: (id, labels) => invoke('set_labels', { id, labels }),
  setIgnored: (id, ignored) => invoke('set_ignored', { id, ignored }),
  removeVolume: (id) => invoke('remove_volume', { id }),
  getSettings: () => invoke('get_settings'),
  setSettings: (settings) => invoke('set_settings', { settings }),
  setTrayLabels: (open, quit) => invoke('set_tray_labels', { open, quit }),
  onVolumesChanged: (handler) => listen('volumes-changed', () => handler()),
  onJobsChanged: (handler) => listen<JobView[]>('jobs-changed', (e) => handler(e.payload)),
};

export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export const backend: Backend = isTauri() ? tauriBackend : mockBackend();

function isApiError(e: unknown): e is ApiError {
  return typeof e === 'object' && e !== null && 'code' in e && 'message' in e;
}

/** A translated message for a backend error (F20-AC3); details go to the console. */
export function errorMessage(e: unknown): string {
  console.error(e);
  if (isApiError(e)) {
    const text = t(`errors.${e.code}`);
    return text === `errors.${e.code}` ? t('errors.unknown') : text;
  }
  return t('errors.unknown');
}
