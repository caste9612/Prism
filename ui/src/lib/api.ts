// Access to the backend: Tauri commands and events, with the types generated from Rust
// (ADR-0014). Outside Tauri (browser development, tests) a simulated backend is used.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ApiError } from './bindings/ApiError';
import type { AppInfo } from './bindings/AppInfo';
import type { CleanupResultView } from './bindings/CleanupResultView';
import type { CleanupView } from './bindings/CleanupView';
import type { JobView } from './bindings/JobView';
import type { LabelsInput } from './bindings/LabelsInput';
import type { LargeFileView } from './bindings/LargeFileView';
import type { NativeTexts } from './bindings/NativeTexts';
import type { Settings } from './bindings/Settings';
import type { SpaceFolderView } from './bindings/SpaceFolderView';
import type { TypeStatsView } from './bindings/TypeStatsView';
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
  openVolume(id: number): Promise<void>;
  spaceFolder(volumeId: number, folderId: number | null): Promise<SpaceFolderView>;
  largestFiles(volumeId: number, folderId: number | null, limit: number): Promise<LargeFileView[]>;
  typeStats(volumeId: number | null): Promise<TypeStatsView>;
  openEntry(volumeId: number, entryId: number): Promise<void>;
  cleanupScan(): Promise<CleanupView>;
  deleteEntries(volumeId: number, entryIds: number[], emptyFolders: boolean, permanent: boolean): Promise<CleanupResultView>;
  revealEntry(volumeId: number, entryId: number): Promise<void>;
  removeVolume(id: number): Promise<void>;
  getSettings(): Promise<Settings>;
  setSettings(settings: Settings): Promise<void>;
  setNativeTexts(texts: NativeTexts): Promise<void>;
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
  openVolume: (id) => invoke('open_volume', { id }),
  spaceFolder: (volumeId, folderId) => invoke('space_folder', { volumeId, folderId }),
  largestFiles: (volumeId, folderId, limit) => invoke('largest_files', { volumeId, folderId, limit }),
  typeStats: (volumeId) => invoke('type_stats', { volumeId }),
  openEntry: (volumeId, entryId) => invoke('open_entry', { volumeId, entryId }),
  cleanupScan: () => invoke('cleanup_scan'),
  deleteEntries: (volumeId, entryIds, emptyFolders, permanent) =>
    invoke('delete_entries', { volumeId, entryIds, emptyFolders, permanent }),
  revealEntry: (volumeId, entryId) => invoke('reveal_entry', { volumeId, entryId }),
  removeVolume: (id) => invoke('remove_volume', { id }),
  getSettings: () => invoke('get_settings'),
  setSettings: (settings) => invoke('set_settings', { settings }),
  setNativeTexts: (texts) => invoke('set_native_texts', { texts }),
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
