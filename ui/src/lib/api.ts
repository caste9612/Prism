// Access to the backend: Tauri commands and events, with the types generated from Rust
// (ADR-0014). Outside Tauri (browser development, tests) a simulated backend is used.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ActionView } from './bindings/ActionView';
import type { AmbiguousView } from './bindings/AmbiguousView';
import type { ApiError } from './bindings/ApiError';
import type { AppInfo } from './bindings/AppInfo';
import type { CleanupResultView } from './bindings/CleanupResultView';
import type { CompareSummaryView } from './bindings/CompareSummaryView';
import type { CopyPreviewView } from './bindings/CopyPreviewView';
import type { CopyResultView } from './bindings/CopyResultView';
import type { DiagnosticsView } from './bindings/DiagnosticsView';
import type { DiskHealthView } from './bindings/DiskHealthView';
import type { CleanupView } from './bindings/CleanupView';
import type { DupFilterInput } from './bindings/DupFilterInput';
import type { DuplicatesView } from './bindings/DuplicatesView';
import type { JobView } from './bindings/JobView';
import type { LabelsInput } from './bindings/LabelsInput';
import type { LargeFileView } from './bindings/LargeFileView';
import type { LocatedView } from './bindings/LocatedView';
import type { NasHealthView } from './bindings/NasHealthView';
import type { NasServerView } from './bindings/NasServerView';
import type { NativeTexts } from './bindings/NativeTexts';
import type { ReportFilterInput } from './bindings/ReportFilterInput';
import type { ReportFolderView } from './bindings/ReportFolderView';
import type { RuleInput } from './bindings/RuleInput';
import type { SearchResultView } from './bindings/SearchResultView';
import type { SharePathView } from './bindings/SharePathView';
import type { ShellRequest } from './bindings/ShellRequest';
import type { SideInput } from './bindings/SideInput';
import type { SimilarView } from './bindings/SimilarView';
import type { Settings } from './bindings/Settings';
import type { SpaceFolderView } from './bindings/SpaceFolderView';
import type { TypeStatsView } from './bindings/TypeStatsView';
import type { UpdateView } from './bindings/UpdateView';
import type { VerifySummaryView } from './bindings/VerifySummaryView';
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
  /** Adds a network folder by its path, after checking that it answers (F02-AC5); its id. */
  addShare(path: string): Promise<number>;
  /** Whether a network share of the catalog answers at a new path, and how much matches (REVIEW D59). */
  checkSharePath(id: number, path: string): Promise<SharePathView>;
  /** Gives a network share of the catalog its new path, after the user confirmed it. */
  moveShare(id: number, path: string): Promise<void>;
  openVolume(id: number): Promise<void>;
  spaceFolder(volumeId: number, folderId: number | null): Promise<SpaceFolderView>;
  largestFiles(volumeId: number, folderId: number | null, limit: number): Promise<LargeFileView[]>;
  typeStats(volumeId: number | null): Promise<TypeStatsView>;
  /** The most recent records of the actions log, newest first (F16-AC6). */
  recentActions(limit: number): Promise<ActionView[]>;
  openEntry(volumeId: number, entryId: number): Promise<void>;
  search(text: string, utcOffsetMin: number): Promise<SearchResultView>;
  hideSearch(): Promise<void>;
  onSearchFocus(handler: () => void): Promise<Unlisten>;
  cleanupScan(): Promise<CleanupView>;
  deleteEntries(volumeId: number, entryIds: number[], emptyFolders: boolean, permanent: boolean): Promise<CleanupResultView>;
  revealEntry(volumeId: number, entryId: number): Promise<void>;
  duplicateGroups(filter: DupFilterInput, rules: RuleInput[]): Promise<DuplicatesView>;
  deleteDuplicates(entryIds: number[], allowOfflineOnly: boolean, permanent: boolean): Promise<CleanupResultView>;
  backupVerify(source: SideInput, destinations: SideInput[]): Promise<VerifySummaryView>;
  backupCompare(a: SideInput, b: SideInput, byContent: boolean): Promise<CompareSummaryView>;
  backupFolder(reportId: number, folder: string, filter: ReportFilterInput): Promise<ReportFolderView>;
  copyPreview(reportId: number, dest: SideInput): Promise<CopyPreviewView>;
  copyStart(reportId: number, dest: SideInput, renameOnConflict: boolean): Promise<number>;
  copyResult(jobId: number): Promise<CopyResultView | null>;
  similarImages(maxDistance: number, volumeId: number | null): Promise<SimilarView>;
  /** A URL for the thumbnail of a connected image, or null. */
  imageThumbnail(volumeId: number, entryId: number): Promise<string | null>;
  similarVideos(maxDistance: number, volumeId: number | null): Promise<SimilarView>;
  /** A URL for the thumbnail of a connected video (its middle frame), or null. */
  videoThumbnail(volumeId: number, entryId: number): Promise<string | null>;
  openLogs(): Promise<void>;
  diagnosticsPreview(): Promise<DiagnosticsView>;
  /** Writes the package and returns its path. */
  exportDiagnostics(): Promise<string>;
  /** Empties the catalog; returns the number of disks removed. */
  resetCatalog(): Promise<number>;
  ambiguousVolumes(): Promise<AmbiguousView[]>;
  diskHealth(volumeId: number): Promise<DiskHealthView | null>;
  /** The NAS of the catalog and whether Prism has their login (REVIEW D66). */
  nasServers(): Promise<NasServerView[]>;
  /** Checks the login by reading the NAS, then keeps it in the Windows Credential Manager. */
  nasConnect(server: string, user: string, password: string): Promise<void>;
  nasDisconnect(server: string): Promise<void>;
  /** The array and the disks of the NAS behind a network share. */
  nasHealth(volumeId: number): Promise<NasHealthView | null>;
  /** The catalog folder of a path on a connected disk (Explorer menu, F18). */
  locatePath(path: string): Promise<LocatedView>;
  onShellRequest(handler: (r: ShellRequest) => void): Promise<Unlisten>;
  updateStatus(): Promise<UpdateView>;
  checkUpdates(): Promise<UpdateView>;
  /** Installs the downloaded update; Prism closes and starts again. */
  installUpdate(): Promise<void>;
  onUpdateChanged(handler: () => void): Promise<Unlisten>;
  /** Which catalog disk an ambiguous disk is, or null for a new one. */
  resolveAmbiguous(key: string, volumeId: number | null): Promise<void>;
  removeVolume(id: number): Promise<void>;
  getSettings(): Promise<Settings>;
  setSettings(settings: Settings): Promise<void>;
  setNativeTexts(texts: NativeTexts): Promise<void>;
  onVolumesChanged(handler: () => void): Promise<Unlisten>;
  onSettingsChanged(handler: () => void): Promise<Unlisten>;
  onJobsChanged(handler: (jobs: JobView[]) => void): Promise<Unlisten>;
}

/** A thumbnail sent as raw JPEG bytes, as an object URL; null when it cannot be made. */
async function thumbnail(command: string, volumeId: number, entryId: number): Promise<string | null> {
  try {
    const jpeg = await invoke<ArrayBuffer>(command, { volumeId, entryId });
    return URL.createObjectURL(new Blob([jpeg], { type: 'image/jpeg' }));
  } catch {
    return null;
  }
}

const tauriBackend: Backend = {
  appInfo: () => invoke('app_info'),
  volumes: () => invoke('get_volumes'),
  jobs: () => invoke('get_jobs'),
  indexVolume: (id) => invoke('index_volume', { id }),
  cancelJob: (id) => invoke('cancel_job', { id }),
  setLabels: (id, labels) => invoke('set_labels', { id, labels }),
  setIgnored: (id, ignored) => invoke('set_ignored', { id, ignored }),
  addShare: (path) => invoke('add_share', { path }),
  checkSharePath: (id, path) => invoke('check_share_path', { id, path }),
  moveShare: (id, path) => invoke('move_share', { id, path }),
  openVolume: (id) => invoke('open_volume', { id }),
  spaceFolder: (volumeId, folderId) => invoke('space_folder', { volumeId, folderId }),
  largestFiles: (volumeId, folderId, limit) => invoke('largest_files', { volumeId, folderId, limit }),
  typeStats: (volumeId) => invoke('type_stats', { volumeId }),
  recentActions: (limit) => invoke('recent_actions', { limit }),
  openEntry: (volumeId, entryId) => invoke('open_entry', { volumeId, entryId }),
  search: (text, utcOffsetMin) => invoke('search', { text, utcOffsetMin }),
  hideSearch: () => invoke('hide_search'),
  onSearchFocus: (handler) => listen('search-focus', () => handler()),
  cleanupScan: () => invoke('cleanup_scan'),
  deleteEntries: (volumeId, entryIds, emptyFolders, permanent) =>
    invoke('delete_entries', { volumeId, entryIds, emptyFolders, permanent }),
  revealEntry: (volumeId, entryId) => invoke('reveal_entry', { volumeId, entryId }),
  duplicateGroups: (filter, rules) => invoke('duplicate_groups', { filter, rules }),
  deleteDuplicates: (entryIds, allowOfflineOnly, permanent) =>
    invoke('delete_duplicates', { entryIds, allowOfflineOnly, permanent }),
  backupVerify: (source, destinations) => invoke('backup_verify', { source, destinations }),
  backupCompare: (a, b, byContent) => invoke('backup_compare', { a, b, byContent }),
  backupFolder: (reportId, folder, filter) => invoke('backup_folder', { reportId, folder, filter }),
  copyPreview: (reportId, dest) => invoke('copy_preview', { reportId, dest }),
  copyStart: (reportId, dest, renameOnConflict) => invoke('copy_start', { reportId, dest, renameOnConflict }),
  copyResult: (jobId) => invoke('copy_result', { jobId }),
  similarImages: (maxDistance, volumeId) => invoke('similar_images', { maxDistance, volumeId }),
  imageThumbnail: (volumeId, entryId) => thumbnail('image_thumbnail', volumeId, entryId),
  similarVideos: (maxDistance, volumeId) => invoke('similar_videos', { maxDistance, volumeId }),
  videoThumbnail: (volumeId, entryId) => thumbnail('video_thumbnail', volumeId, entryId),
  openLogs: () => invoke('open_logs'),
  diagnosticsPreview: () => invoke('diagnostics_preview'),
  exportDiagnostics: () => invoke('export_diagnostics'),
  resetCatalog: () => invoke('reset_catalog'),
  ambiguousVolumes: () => invoke('ambiguous_volumes'),
  diskHealth: (volumeId) => invoke('disk_health', { volumeId }),
  nasServers: () => invoke('nas_servers'),
  nasConnect: (server, user, password) => invoke('nas_connect', { server, user, password }),
  nasDisconnect: (server) => invoke('nas_disconnect', { server }),
  nasHealth: (volumeId) => invoke('nas_health', { volumeId }),
  locatePath: (path) => invoke('locate_path', { path }),
  onShellRequest: (handler) => listen<ShellRequest>('shell-request', (e) => handler(e.payload)),
  updateStatus: () => invoke('update_status'),
  checkUpdates: () => invoke('check_updates'),
  installUpdate: () => invoke('install_update'),
  onUpdateChanged: (handler) => listen('update-changed', () => handler()),
  resolveAmbiguous: (key, volumeId) => invoke('resolve_ambiguous', { key, volumeId }),
  removeVolume: (id) => invoke('remove_volume', { id }),
  getSettings: () => invoke('get_settings'),
  setSettings: (settings) => invoke('set_settings', { settings }),
  setNativeTexts: (texts) => invoke('set_native_texts', { texts }),
  onVolumesChanged: (handler) => listen('volumes-changed', () => handler()),
  onSettingsChanged: (handler) => listen('settings-changed', () => handler()),
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
