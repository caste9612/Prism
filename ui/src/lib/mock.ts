// A simulated backend with sample disks, used when the interface runs outside Tauri (browser
// development, tests, screenshots). It behaves like the real one: indexing jobs progress and
// emit events, labels and settings are kept in memory.
import type { AppInfo } from './bindings/AppInfo';
import type { JobView } from './bindings/JobView';
import type { LabelsInput } from './bindings/LabelsInput';
import type { Settings } from './bindings/Settings';
import type { VolumeView } from './bindings/VolumeView';
import type { Backend, Unlisten } from './api';
import { cardJob } from './catalog';
import { mockCleanupScan, mockDeleteEntries } from './mock-cleanup';
import { mockDeleteDuplicates, mockDuplicates } from './mock-duplicates';
import { FAILING_COPY, mockBackup } from './mock-backup';
import type { CopyResultView } from './bindings/CopyResultView';
import { mockSearch } from './mock-search';
import { mockHasEntry, mockLargestFiles, mockSpaceFolder, mockTypeStats } from './mock-space';

const GB = 1_000_000_000;
const DAY = 86_400_000;

function volume(v: Partial<VolumeView> & Pick<VolumeView, 'id' | 'name' | 'kind'>): VolumeView {
  return {
    userName: null,
    location: null,
    notes: null,
    color: null,
    osLabel: null,
    fsType: 'NTFS',
    mount: null,
    online: true,
    status: 'indexed',
    capacity: null,
    freeSpace: null,
    indexedSize: 0,
    fileCount: 0,
    folderCount: 0,
    lastIndexedAt: null,
    lastSeenAt: null,
    job: null,
    hashes: null,
    ...v,
  };
}

export function sampleVolumes(now = Date.now()): VolumeView[] {
  return [
    volume({ id: 1, name: 'Windows', osLabel: 'Windows', kind: 'internal', mount: 'C:\\', capacity: 439 * GB, freeSpace: 184 * GB, indexedSize: 210 * GB, fileCount: 951_644, folderCount: 307_861, lastIndexedAt: now - 3 * 60_000, lastSeenAt: now, hashes: { total: 900_000, hashed: 412_000, running: true } }),
    volume({ id: 2, name: 'Data', osLabel: 'Data', kind: 'internal', mount: 'D:\\', capacity: 977 * GB, freeSpace: 329 * GB, indexedSize: 612 * GB, fileCount: 544_808, folderCount: 76_107, lastIndexedAt: now - 2 * 3_600_000, lastSeenAt: now, hashes: { total: 500_000, hashed: 500_000, running: false } }),
    volume({ id: 3, name: 'Backup Foto', userName: 'Backup Foto', location: 'cassetto 2', color: '#f59e0b', kind: 'external', mount: 'G:\\', online: false, capacity: 2000 * GB, freeSpace: 700 * GB, indexedSize: 1240 * GB, fileCount: 182_340, folderCount: 9_811, lastIndexedAt: now - 12 * DAY, lastSeenAt: now - 12 * DAY }),
    volume({ id: 4, name: 'NAS Big-Storage', userName: 'NAS Big-Storage', kind: 'network', fsType: null, mount: 'Y:\\', indexedSize: 634 * GB, fileCount: 187_683, folderCount: 12_400, lastIndexedAt: now - DAY, lastSeenAt: now }),
    volume({ id: 5, name: 'KINGSTON', osLabel: 'KINGSTON', kind: 'stick', fsType: 'exFAT', mount: 'H:\\', status: 'new', capacity: 64 * GB, freeSpace: 51 * GB, lastSeenAt: now }),
  ];
}

export function mockBackend(): Backend {
  let volumes = sampleVolumes();
  let settings: Settings = {
    language: 'system',
    theme: 'system',
    autostart: true,
    searchShortcut: 'Ctrl+Space',
    hashingPaused: false,
    pauseHashingOnBattery: false,
    hashingExcluded: [],
  };
  const deletedCopies = new Set<number>();
  const backup = mockBackup(() => volumes);
  const copyResults = new Map<number, CopyResultView>();
  const settingsListeners = new Set<() => void>();
  const jobs: JobView[] = [];
  let nextJob = 1;
  const volumeListeners = new Set<() => void>();
  const jobListeners = new Set<(jobs: JobView[]) => void>();

  const emitVolumes = () => volumeListeners.forEach((h) => h());
  const emitJobs = () => jobListeners.forEach((h) => h(jobs.map((j) => ({ ...j }))));
  const find = (id: number) => {
    const v = volumes.find((x) => x.id === id);
    if (!v) throw { code: 'volume_not_found', message: String(id) };
    return v;
  };
  const withJobs = () =>
    volumes.map((v) => ({
      ...v,
      job: cardJob(jobs, v.id),
    }));

  const on = <T>(set: Set<T>, h: T): Promise<Unlisten> => {
    set.add(h);
    return Promise.resolve(() => set.delete(h));
  };

  return {
    appInfo: async (): Promise<AppInfo> => ({ version: '0.1.0', elevated: true, dataDir: '(simulated)', dev: true, shortcutError: null }),
    volumes: async () => withJobs(),
    jobs: async () => jobs.map((j) => ({ ...j })),
    indexVolume: async (id) => {
      const v = find(id);
      if (!v.online) throw { code: 'volume_offline', message: String(id) };
      const job: JobView = { id: nextJob++, kind: v.status === 'indexed' ? 'update' : 'index', volumeId: id, state: 'running', done: 0, total: 0, phase: 'scanning', error: null };
      jobs.push(job);
      v.status = 'indexing';
      emitVolumes();
      const timer = setInterval(() => {
        if (job.state !== 'running') {
          clearInterval(timer);
          return;
        }
        job.done += 25_000;
        if (job.done >= 150_000) {
          job.state = 'done';
          v.status = 'indexed';
          v.fileCount = 120_000;
          v.folderCount = 9_000;
          v.indexedSize = (v.capacity ?? 0) - (v.freeSpace ?? 0);
          v.lastIndexedAt = Date.now();
          clearInterval(timer);
          emitVolumes();
        }
        emitJobs();
      }, 300);
      return job.id;
    },
    cancelJob: async (id) => {
      const job = jobs.find((j) => j.id === id);
      if (!job || job.state !== 'running') return false;
      job.state = 'cancelled';
      const v = volumes.find((x) => x.id === job.volumeId);
      if (v && (job.kind === 'index' || job.kind === 'update')) v.status = 'reindex_needed';
      emitJobs();
      emitVolumes();
      return true;
    },
    setLabels: async (id, labels: LabelsInput) => {
      const v = find(id);
      v.userName = labels.name;
      v.location = labels.location;
      v.notes = labels.notes;
      v.color = labels.color;
      v.name = [labels.name, v.osLabel].find((n) => n !== null && n.trim() !== '') ?? v.mount ?? `#${id}`;
      emitVolumes();
    },
    setIgnored: async (id, ignored) => {
      const v = find(id);
      v.status = ignored ? 'ignored' : v.lastIndexedAt ? 'reindex_needed' : 'new';
      emitVolumes();
    },
    openVolume: async (id) => {
      if (!find(id).online) throw { code: 'volume_offline', message: String(id) };
    },
    spaceFolder: async (volumeId, folderId) => mockSpaceFolder(find(volumeId).id, folderId),
    largestFiles: async (volumeId, folderId, limit) => mockLargestFiles(find(volumeId).id, folderId, limit),
    typeStats: async (volumeId) =>
      mockTypeStats(volumeId === null ? volumes.filter((v) => v.lastIndexedAt !== null).map((v) => v.id) : [find(volumeId).id]),
    openEntry: async (volumeId, entryId) => {
      if (!find(volumeId).online) throw { code: 'volume_offline', message: String(volumeId) };
      if (!mockHasEntry(volumeId, entryId)) throw { code: 'not_found', message: String(entryId) };
    },
    search: async (text) => mockSearch(text, withJobs()),
    hideSearch: async () => {},
    onSearchFocus: async () => () => {},
    cleanupScan: async () => mockCleanupScan(),
    deleteEntries: async (volumeId, entryIds, _emptyFolders, permanent) => {
      if (!find(volumeId).online) throw { code: 'volume_offline', message: String(volumeId) };
      const result = mockDeleteEntries(volumeId, entryIds, permanent);
      emitVolumes();
      return result;
    },
    duplicateGroups: async (filter, rules) => mockDuplicates(volumes, filter, rules, deletedCopies),
    deleteDuplicates: async (ids, allowOfflineOnly, permanent) => {
      const result = mockDeleteDuplicates(volumes, ids, allowOfflineOnly, permanent, deletedCopies);
      emitVolumes();
      return result;
    },
    backupVerify: async (source, destinations) => backup.verify(source, destinations),
    backupCompare: async (a, b, byContent) => backup.compare(a, b, byContent),
    backupFolder: async (reportId, folder, filter) => backup.folder(reportId, folder, filter),
    copyPreview: async (reportId, dest) => backup.preview(reportId, dest),
    copyStart: async (reportId, dest) => {
      const preview = backup.preview(reportId, dest);
      const job: JobView = { id: nextJob++, kind: 'copy', volumeId: dest.volumeId, state: 'running', done: 0, total: preview.bytes, phase: 'copying', error: null };
      jobs.push(job);
      emitJobs();
      const step = preview.bytes / 4;
      const timer = setInterval(() => {
        if (job.state === 'cancelled') {
          clearInterval(timer);
          copyResults.set(job.id, { copied: Math.floor((job.done / preview.bytes) * preview.files), renamed: 0, alreadyThere: 0, skipped: 0, failed: [], bytes: job.done, interrupted: true });
          emitJobs();
          return;
        }
        job.done = Math.min(preview.bytes, job.done + step);
        if (job.done >= preview.bytes) {
          clearInterval(timer);
          backup.copied(reportId);
          copyResults.set(job.id, { copied: preview.files - 1, renamed: 0, alreadyThere: 0, skipped: 0, failed: [{ path: FAILING_COPY, error: 'Access denied (os error 5)' }], bytes: preview.bytes, interrupted: false });
          job.state = 'done';
        }
        emitJobs();
      }, 400);
      return job.id;
    },
    copyResult: async (jobId) => copyResults.get(jobId) ?? null,
    revealEntry: async (volumeId, entryId) => {
      if (!find(volumeId).online) throw { code: 'volume_offline', message: String(volumeId) };
      if (!mockHasEntry(volumeId, entryId)) throw { code: 'not_found', message: String(entryId) };
    },
    removeVolume: async (id) => {
      volumes = volumes.filter((v) => v.id !== id);
      emitVolumes();
    },
    getSettings: async () => ({ ...settings }),
    setSettings: async (s) => {
      settings = { ...s };
    },
    setNativeTexts: async () => {},
    onVolumesChanged: (h) => on(volumeListeners, h),
    onSettingsChanged: (h) => on(settingsListeners, h),
    onJobsChanged: (h) => on(jobListeners, h),
  };
}
