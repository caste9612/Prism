<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { save } from '@tauri-apps/plugin-dialog';
  import { stats, searchStore, duplicatesStore, analyticsStore, settingsStore, notificationStore, notifications } from '$lib/stores';
  import { formatBytes, formatNumber } from '$lib/utils/format';
  import { onMount, onDestroy } from 'svelte';

  // Components
  import Toast from '$lib/components/common/Toast.svelte';
  import Modal from '$lib/components/common/Modal.svelte';
  import StatsCards from '$lib/components/analytics/StatsCards.svelte';
  import StorageExplorer from '$lib/components/analytics/StorageExplorer.svelte';
  import FileCategories from '$lib/components/analytics/FileCategories.svelte';
  import SizeDistribution from '$lib/components/analytics/SizeDistribution.svelte';
  import DuplicateList from '$lib/components/duplicates/DuplicateList.svelte';

  // Types
  import type { DriveStatus, DriveProgress, ScanProgress as ScanProgressType, DriveStats, TabType, PreScanOverlapResult, VerificationSummary, VerificationProgress, IncrementalProgress } from '$lib/types';

  // Use the centralized notification store
  function notify(message: string, type: 'success' | 'error' | 'info' = 'info') {
    notificationStore.notify(message, type);
  }

  // Drives
  let drives: DriveStatus[] = [];
  let selectedDrives = new Set<string>();
  let loadingDrives = true;
  let driveStats: DriveStats[] = [];
  let driveRefreshInterval: ReturnType<typeof setInterval> | null = null;

  // Per-drive scan progress tracking (from backend)
  let driveProgress: Record<string, DriveProgress> = {};

  // Scan state
  let isScanning = false;
  let isAutoScan = false; // Track if we're in auto-scan mode
  let scanProgress: ScanProgressType | null = null;
  let incrementalProgress: IncrementalProgress | null = null;
  let autoScanPhase: 'local' | 'network' | 'all' | null = null; // Track auto-scan phase
  let autoScanHasNetwork = false; // Track if network drives are pending
  let unlistenProgress: (() => void) | null = null;
  let unlistenComplete: (() => void) | null = null;
  let unlistenStats: (() => void) | null = null;
  let unlistenIncrementalProgress: (() => void) | null = null;
  let unlistenAutoScanPhase: (() => void) | null = null;
  let unlistenAutoScanComplete: (() => void) | null = null;

  // Settings
  $: settings = $settingsStore;
  let tempSettings = { ...settings };

  // Delete confirmation
  let deleteConfirm: { files: { id: number; path: string }[]; show: boolean } = { files: [], show: false };
  let deleting = false;

  // Overlap warning
  let overlapWarning: { show: boolean; results: PreScanOverlapResult[]; pendingPaths: string[] } = {
    show: false,
    results: [],
    pendingPaths: []
  };
  let checkingOverlaps = false;

  onMount(async () => {
    // Listen for scan progress events
    unlistenProgress = await listen<ScanProgressType>('scan-progress', (event) => {
      scanProgress = event.payload;

      // Debug: log all progress events with indexing info
      console.log('Scan progress received:', {
        current_path: scanProgress?.current_path,
        has_indexing: 'indexing' in (scanProgress || {}),
        indexing: scanProgress?.indexing,
        is_complete: scanProgress?.is_complete
      });

      // Update per-drive progress from backend - MERGE with existing data
      // This preserves completed drives from previous phases
      if (scanProgress?.drives) {
        driveProgress = { ...driveProgress, ...scanProgress.drives };
        // Log drives progress every few updates
        const driveKeys = Object.keys(scanProgress.drives);
        if (driveKeys.length > 0) {
          const summary = driveKeys.map(k => `${k}:${Math.round(scanProgress.drives[k]?.progress_percent ?? 0)}%`).join(', ');
          console.log('[driveProgress]', summary);
        }
      }

      if (scanProgress?.is_complete) {
        // During auto-scan, NEVER set isScanning=false here - let auto-scan-complete handle it
        // This prevents race conditions where scan-complete arrives after phase change
        if (!isAutoScan) {
          console.log('[UI] Manual scan complete, setting isScanning=false');
          isScanning = false;
        } else {
          console.log('[UI] Auto-scan phase complete (phase=%s), waiting for auto-scan-complete', autoScanPhase);
        }

        if (scanProgress.error) {
          notify(`Scan failed: ${scanProgress.error}`, 'error');
        } else {
          // Only show "complete" notification for manual scans (auto-scan uses auto-scan-complete)
          if (!isAutoScan) {
            notify(`Scan complete: ${formatNumber(scanProgress.files_scanned)} files indexed`, 'success');
          }
          loadAnalytics();
          loadDriveStats();
        }
      }
    });

    unlistenComplete = await listen('scan-complete', async () => {
      // During auto-scan, ALWAYS ignore scan-complete - let auto-scan-complete handle it
      // This prevents race conditions where scan-complete arrives after phase change
      if (isAutoScan) {
        console.log('[UI] Ignoring scan-complete during auto-scan (phase=%s)', autoScanPhase);
        return;
      }
      console.log('[UI] scan-complete received (manual scan), setting isScanning=false');
      isScanning = false;
      await stats.load();
      loadAnalytics();
      loadDriveStats();
    });

    unlistenStats = await listen('stats-updated', async () => {
      await stats.load();
    });

    // Load drives first
    await loadDrives();

    // Load existing analytics
    await loadAnalytics();
    await loadDriveStats();

    // Start auto-scan if there are ready local drives
    if (drives.some(d => d.is_ready && d.drive_type === 'Local Disk')) {
      notify('Starting automatic scan of all drives...', 'info');
      await startAutoScan();
    }

    // Auto-refresh drives every 15 seconds to detect newly connected drives
    driveRefreshInterval = setInterval(() => {
      if (!isScanning) {
        loadDrives(true); // silent refresh
      }
    }, 15000);

    // Listen for verification progress
    unlistenVerifyProgress = await listen<VerificationProgress>('verify-progress', (event) => {
      verifyProgress = event.payload;
    });

    // Listen for incremental scan progress
    unlistenIncrementalProgress = await listen<IncrementalProgress>('incremental-progress', (event) => {
      incrementalProgress = event.payload;
      console.log('[UI] Incremental progress:', incrementalProgress?.phase, 'percent:', incrementalProgress?.percent?.toFixed(1),
                  'autoScanPhase:', autoScanPhase, 'isAutoScan:', isAutoScan);

      // When incremental scan completes
      if (incrementalProgress?.phase === 'complete') {
        if (isAutoScan) {
          // During auto-scan, DON'T reset incrementalProgress here
          // The auto-scan-phase handler will reset it when switching phases
          // And auto-scan-complete will reset it at the end
          console.log('[UI] Auto-scan phase complete (phase=%s), waiting for next phase...', autoScanPhase);
        } else {
          // Manual scan completion - reset everything after a brief delay
          console.log('[UI] Manual scan complete, resetting...');
          setTimeout(() => {
            incrementalProgress = null;
            isScanning = false;
            loadAnalytics();
            loadDriveStats();
          }, 1000);
        }
      }
    });

    // Listen for auto-scan phase changes
    unlistenAutoScanPhase = await listen<{ phase: string; has_network: boolean; paths: string[] }>('auto-scan-phase', (event) => {
      console.log('[UI] Auto-scan phase received:', event.payload.phase, 'paths:', event.payload.paths.length);
      autoScanPhase = event.payload.phase as 'local' | 'network' | 'all';
      autoScanHasNetwork = event.payload.has_network;

      // IMPORTANT: Set isScanning = true when scan starts
      isScanning = true;
      console.log('[UI] After phase change: isScanning=%s, autoScanPhase=%s', isScanning, autoScanPhase);

      // Reset progress for new scan
      incrementalProgress = null;
      driveProgress = {};
    });

    // Listen for auto-scan complete
    unlistenAutoScanComplete = await listen<{ results: string }>('auto-scan-complete', async (event) => {
      console.log('[UI] Auto-scan complete:', event.payload);
      autoScanPhase = null;
      autoScanHasNetwork = false;
      isAutoScan = false;
      isScanning = false;
      incrementalProgress = null;

      // Load fresh stats and show completion notification
      await stats.load();
      const totalFiles = $stats?.totalFiles ?? 0;
      notify(`All drives scanned: ${formatNumber(totalFiles)} files indexed`, 'success');

      loadAnalytics();
      loadDriveStats();
    });
  });

  onDestroy(() => {
    unlistenProgress?.();
    unlistenComplete?.();
    unlistenStats?.();
    unlistenVerifyProgress?.();
    unlistenIncrementalProgress?.();
    unlistenAutoScanPhase?.();
    unlistenAutoScanComplete?.();
    if (driveRefreshInterval) {
      clearInterval(driveRefreshInterval);
      driveRefreshInterval = null;
    }
  });

  async function loadDrives(silent = false) {
    if (!silent) loadingDrives = true;
    try {
      const previousDrives = new Map(drives.map(d => [d.path, d.is_online]));
      drives = await invoke<DriveStatus[]>('get_drives_status');

      // Auto-select online local disks that haven't been scanned yet (only on initial load)
      if (!silent) {
        drives.forEach(d => {
          if (d.is_online && d.drive_type === 'Local Disk' && d.is_ready) {
            selectedDrives.add(d.path);
          }
        });
        selectedDrives = selectedDrives;
      }

      // Notify about drives that came online or went offline
      for (const drive of drives) {
        const wasOnline = previousDrives.get(drive.path);
        if (wasOnline !== undefined && wasOnline !== drive.is_online) {
          if (drive.is_online) {
            notify(`${drive.name} is now online`, 'info');
          } else {
            notify(`${drive.name} went offline`, 'info');
          }
        }
      }
    } catch (e) {
      console.error('Failed to load drives:', e);
      if (!silent) notify('Failed to detect drives', 'error');
    } finally {
      loadingDrives = false;
    }
  }

  async function loadDriveStats() {
    try {
      driveStats = await invoke<DriveStats[]>('get_drive_stats');
    } catch (e) {
      console.error('Failed to load drive stats:', e);
    }
  }

  async function removeOfflineDrive(path: string) {
    try {
      const [filesDeleted, sizeDeleted] = await invoke<[number, number]>('remove_offline_drive', { drivePath: path });
      notify(`Removed offline drive ${path}: ${formatNumber(filesDeleted)} files (${formatBytes(sizeDeleted)})`, 'success');
      await loadDrives();
      await stats.load();
      await loadDriveStats();
    } catch (e) {
      console.error('Failed to remove offline drive:', e);
      notify(`Failed to remove offline drive: ${e}`, 'error');
    }
  }

  function toggleDrive(path: string) {
    const drive = drives.find(d => d.path === path);
    if (selectedDrives.has(path)) {
      selectedDrives.delete(path);
    } else {
      selectedDrives.add(path);
      // Warn about network drives potentially containing duplicates
      if (drive?.drive_type === 'Network Drive') {
        notify('Warning: Network drives may contain duplicates of local files', 'info');
      }
    }
    selectedDrives = selectedDrives;
  }

  async function startAutoScan() {
    isScanning = true;
    isAutoScan = true; // Mark as auto-scan mode
    driveProgress = {};
    scanProgress = { scan_id: 0, files_scanned: 0, total_size: 0, current_path: 'Initializing...', files_per_second: 0, is_complete: false, error: null, drives: {} };

    try {
      await invoke('auto_scan_drives');
    } catch (e) {
      notify(`Auto-scan failed: ${e}`, 'error');
      isScanning = false;
      isAutoScan = false;
    }
  }

  async function startScan() {
    if (selectedDrives.size === 0) return;

    // Check if any drives have been scanned before (to check for overlaps)
    const hasScannedDrives = drives.some(d => d.is_scanned);

    if (hasScannedDrives) {
      // Check for overlaps before scanning
      checkingOverlaps = true;
      const overlappingResults: PreScanOverlapResult[] = [];

      for (const drivePath of selectedDrives) {
        const drive = drives.find(d => d.path === drivePath);
        // Only check drives that haven't been scanned yet
        if (drive && !drive.is_scanned) {
          try {
            const result = await invoke<PreScanOverlapResult>('check_pre_scan_overlap', {
              drivePath,
              sampleSize: 50
            });
            if (result.recommendation !== 'scan') {
              overlappingResults.push(result);
            }
          } catch (e) {
            console.warn(`Failed to check overlap for ${drivePath}:`, e);
          }
        }
      }

      checkingOverlaps = false;

      // Show warning if overlapping drives found
      if (overlappingResults.length > 0) {
        overlapWarning = {
          show: true,
          results: overlappingResults,
          pendingPaths: [...selectedDrives]
        };
        return; // Wait for user decision
      }
    }

    // No overlaps or no previously scanned drives, proceed with scan
    await executeScan([...selectedDrives]);
  }

  async function executeScan(paths: string[]) {
    if (paths.length === 0) return;
    isScanning = true;
    driveProgress = {};
    scanProgress = { scan_id: 0, files_scanned: 0, total_size: 0, current_path: 'Starting...', files_per_second: 0, is_complete: false, error: null, drives: {} };

    try {
      await invoke('start_scan', {
        request: {
          paths,
          excludeHidden: settings.excludeHidden,
          excludeSystem: settings.excludeSystem,
          excludePatterns: settings.excludePatterns,
          minFileSize: settings.minFileSize,
        }
      });
    } catch (e) {
      notify(`Scan failed: ${e}`, 'error');
      isScanning = false;
    }
  }

  function handleOverlapDecision(skipOverlapping: boolean) {
    const pathsToScan = skipOverlapping
      ? overlapWarning.pendingPaths.filter(p => !overlapWarning.results.some(r => r.drive_path === p))
      : overlapWarning.pendingPaths;

    overlapWarning = { show: false, results: [], pendingPaths: [] };

    if (pathsToScan.length > 0) {
      executeScan(pathsToScan);
    } else {
      notify('No drives to scan after skipping overlapping drives', 'info');
    }
  }

  // Get scan status for a drive
  function getDriveScanStatus(drivePath: string): 'idle' | 'scanning' | 'done' {
    if (!isScanning) return 'idle';
    const normalizedPath = drivePath.toUpperCase();

    // Check per-drive progress
    const progress = driveProgress[normalizedPath];

    // If drive has progress data, use its status
    if (progress) {
      if (progress.status === 'done') return 'done';
      if (progress.status === 'scanning') return 'scanning';
      if (progress.status === 'waiting') return 'scanning'; // waiting = about to scan
    }

    // No progress data yet - show as scanning (waiting for first update)
    return 'scanning';
  }

  // Get indexed file count for a drive
  function getDriveIndexedCount(drivePath: string): number {
    const stat = driveStats.find(s => s.path.toUpperCase() === drivePath.toUpperCase());
    return stat?.file_count || 0;
  }

  function getDriveIndexedSize(drivePath: string): number {
    const stat = driveStats.find(s => s.path.toUpperCase() === drivePath.toUpperCase());
    return stat?.total_size || 0;
  }

  // Stats
  $: totalFiles = $stats.totalFiles;
  $: totalSize = $stats.totalSize;

  // Tabs - Analytics is default
  let activeTab: TabType = 'analytics';
  const tabs: { id: TabType; label: string }[] = [
    { id: 'analytics', label: 'Analytics' },
    { id: 'duplicates', label: 'Duplicates' },
    { id: 'verify', label: 'Verify Backup' },
    { id: 'settings', label: 'Settings' }
  ];

  function switchTab(tab: TabType) {
    activeTab = tab;
    if (tab === 'analytics') loadAnalytics();
    if (tab === 'settings') tempSettings = { ...settings };
  }

  // Search - Always visible
  $: query = $searchStore.query;
  $: searchResults = $searchStore.results;
  $: searchTotal = $searchStore.totalCount;
  $: searchLoading = $searchStore.loading;
  let showSearchResults = false;

  function handleSearch(e: Event) {
    const target = e.target as HTMLInputElement;
    searchStore.setQuery(target.value);
    showSearchResults = target.value.length >= 2;
  }

  function closeSearchResults() {
    showSearchResults = false;
  }

  async function openInExplorer(path: string) {
    try {
      await invoke('open_in_explorer', { path });
    } catch (e) {
      notify(`Failed to open: ${e}`, 'error');
    }
  }

  async function exportCsv() {
    const path = await save({ defaultPath: 'prism-export.csv', filters: [{ name: 'CSV', extensions: ['csv'] }] });
    if (path) {
      try {
        const count = await invoke<number>('export_to_csv', { outputPath: path, query: query.length >= 2 ? query : null });
        notify(`Exported ${formatNumber(count)} files to CSV`, 'success');
      } catch (e) {
        notify(`Export failed: ${e}`, 'error');
      }
    }
  }

  // Duplicates
  $: dupGroups = $duplicatesStore.groups;
  $: dupLoading = $duplicatesStore.loading;
  $: wastedSpace = $duplicatesStore.totalWastedSpace;
  let expandedDupGroups = new Set<number>();
  let selectedDupFiles = new Set<string>();

  async function findDuplicates() {
    selectedDupFiles = new Set();
    await duplicatesStore.findDuplicates();
  }

  // Verification state
  let verifySourceDrive = '';
  let verifyTargetDrives = new Set<string>();
  let verifyLoading = false;
  let verifyResult: VerificationSummary | null = null;
  let verifyProgress: VerificationProgress | null = null;
  let unlistenVerifyProgress: (() => void) | null = null;

  function toggleVerifyTarget(path: string) {
    if (verifyTargetDrives.has(path)) {
      verifyTargetDrives.delete(path);
    } else {
      verifyTargetDrives.add(path);
    }
    verifyTargetDrives = verifyTargetDrives;
  }

  async function runVerification() {
    if (!verifySourceDrive || verifyTargetDrives.size === 0) return;

    verifyLoading = true;
    verifyResult = null;
    verifyProgress = null;

    try {
      const result = await invoke<VerificationSummary>('verify_cross_disk', {
        sourceDrive: verifySourceDrive,
        targetDrives: [...verifyTargetDrives],
        maxMissingFiles: 100
      });
      verifyResult = result;

      if (result.backupPercentage >= 100) {
        notify('All files are backed up!', 'success');
      } else if (result.backupPercentage >= 90) {
        notify(`${result.backupPercentage.toFixed(1)}% of files backed up. ${result.filesMissing} files missing.`, 'info');
      } else {
        notify(`Warning: Only ${result.backupPercentage.toFixed(1)}% backed up. ${result.filesMissing} files missing.`, 'error');
      }
    } catch (e) {
      notify(`Verification failed: ${e}`, 'error');
    } finally {
      verifyLoading = false;
    }
  }

  function toggleDupGroup(id: number) {
    if (expandedDupGroups.has(id)) expandedDupGroups.delete(id);
    else expandedDupGroups.add(id);
    expandedDupGroups = expandedDupGroups;
  }

  function toggleDupFile(id: number, path: string) {
    const key = `${id}:${path}`;
    if (selectedDupFiles.has(key)) selectedDupFiles.delete(key);
    else selectedDupFiles.add(key);
    selectedDupFiles = selectedDupFiles;
  }

  function selectAllDuplicates() {
    dupGroups.forEach(group => {
      group.files.slice(1).forEach(file => {
        selectedDupFiles.add(`${file.id}:${file.path}`);
      });
    });
    selectedDupFiles = selectedDupFiles;
  }

  function showDeleteConfirm() {
    const files = [...selectedDupFiles].map(key => {
      const [id, ...pathParts] = key.split(':');
      return { id: parseInt(id), path: pathParts.join(':') };
    });
    deleteConfirm = { files, show: true };
  }

  async function confirmDelete() {
    deleting = true;
    try {
      const filesToDelete = deleteConfirm.files.map(f => [f.id, f.path] as [number, string]);
      const deleted = await invoke<number>('delete_duplicates_batch', { files: filesToDelete });
      notify(`Deleted ${deleted} files`, 'success');
      selectedDupFiles = new Set();
      deleteConfirm = { files: [], show: false };
      await findDuplicates();
    } catch (e) {
      notify(`Delete failed: ${e}`, 'error');
    } finally {
      deleting = false;
    }
  }

  // Analytics
  $: sizeDistribution = $analyticsStore.sizeDistribution;
  $: extensionDistribution = $analyticsStore.extensionDistribution;
  $: fileTypeCategories = $analyticsStore.fileTypeCategories;
  $: folderContents = $analyticsStore.folderContents;
  $: analyticsLoading = $analyticsStore.loading;

  // Treemap navigation state
  let treemapPath: string[] = [];
  let treemapLoading = false;
  let expandedCategory: string | null = null;

  async function loadTreemapLevel(path?: string) {
    treemapLoading = true;
    try {
      const data = await analyticsStore.loadFolderContents(path, 20);
      console.log('Loaded folder contents for path:', path, 'Got:', data?.length, 'items');
    } catch (e) {
      console.error('Failed to load folder contents for path:', path, e);
      notify(`Failed to load folder: ${e}`, 'error');
    } finally {
      treemapLoading = false;
    }
  }

  function drillDownFolder(item: { path: string; name: string }) {
    console.log('Drilling down into:', item);
    treemapPath = [...treemapPath, item.name];
    loadTreemapLevel(item.path);
  }

  function navigateToLevel(index: number) {
    if (index < 0) {
      treemapPath = [];
      loadTreemapLevel();
    } else {
      // Rebuild path from segments
      const segments = treemapPath.slice(0, index + 1);
      treemapPath = segments;
      // Reconstruct the full path
      if (segments.length > 0) {
        const fullPath = segments[0] + '\\' + segments.slice(1).join('\\');
        loadTreemapLevel(fullPath);
      }
    }
  }

  async function loadAnalytics() {
    await analyticsStore.loadAll();
    // Load new analytics data
    await analyticsStore.loadFileTypeDistribution();
    await analyticsStore.loadFolderContents();
  }

  // Clear database
  async function clearDatabase() {
    if (!confirm('Clear all scanned data? This cannot be undone.')) return;
    try {
      await invoke('clear_database');
      await stats.load();
      driveStats = [];
      notify('Database cleared', 'success');
    } catch (e) {
      notify(`Failed to clear: ${e}`, 'error');
    }
  }

  // Settings
  function saveSettings() {
    settingsStore.setAll(tempSettings);
    notify('Settings saved', 'success');
  }

  function addExcludePattern() {
    const pattern = prompt('Enter folder name to exclude:');
    if (pattern && !tempSettings.excludePatterns.includes(pattern)) {
      tempSettings.excludePatterns = [...tempSettings.excludePatterns, pattern];
    }
  }

  function removeExcludePattern(pattern: string) {
    tempSettings.excludePatterns = tempSettings.excludePatterns.filter(p => p !== pattern);
  }

  function getFileIcon(ext: string | null): string {
    if (!ext) return 'text-gray-400';
    const e = ext.toLowerCase();
    if (['jpg', 'jpeg', 'png', 'gif', 'webp'].includes(e)) return 'text-pink-400';
    if (['mp4', 'mkv', 'avi', 'mov'].includes(e)) return 'text-purple-400';
    if (['mp3', 'wav', 'flac'].includes(e)) return 'text-green-400';
    if (['pdf', 'doc', 'docx'].includes(e)) return 'text-blue-400';
    if (['zip', 'rar', '7z'].includes(e)) return 'text-yellow-400';
    return 'text-gray-400';
  }

</script>

<!-- Toast Notifications -->
<Toast notifications={$notifications} />

<!-- Delete Confirmation Modal -->
<Modal
  show={deleteConfirm.show}
  title="Delete {deleteConfirm.files.length} files?"
  description="This action cannot be undone"
  confirmText="Delete Files"
  loading={deleting}
  on:confirm={confirmDelete}
  on:cancel={() => deleteConfirm = { files: [], show: false }}
>
  <div slot="content" class="max-h-40 overflow-y-auto text-sm text-gray-400">
    {#each deleteConfirm.files.slice(0, 5) as file}
      <div class="truncate">{file.path}</div>
    {/each}
    {#if deleteConfirm.files.length > 5}
      <div class="text-gray-500">...and {deleteConfirm.files.length - 5} more</div>
    {/if}
  </div>
</Modal>

<!-- Overlap Warning Modal -->
<Modal
  show={overlapWarning.show}
  title="Overlapping Drives Detected"
  description="Some selected drives appear to contain duplicate files from already-scanned drives"
  confirmText="Skip Overlapping"
  cancelText="Scan Anyway"
  on:confirm={() => handleOverlapDecision(true)}
  on:cancel={() => handleOverlapDecision(false)}
>
  <div slot="content" class="space-y-3">
    {#each overlapWarning.results as result}
      <div class="bg-yellow-900/30 border border-yellow-600/50 rounded-lg p-3">
        <div class="flex items-center gap-2 text-yellow-400 font-medium">
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
          </svg>
          <span>{result.drive_path}</span>
          <span class="ml-auto text-sm">{Math.round(result.overlap_percent)}% overlap</span>
        </div>
        {#if result.overlaps_with.length > 0}
          <p class="text-sm text-gray-400 mt-1">
            Contains files from: {result.overlaps_with.join(', ')}
          </p>
        {/if}
        <p class="text-xs text-gray-500 mt-1">
          {result.recommendation === 'skip' ? 'Strongly recommend skipping' : 'Consider skipping'}
          - scanning may create duplicate entries
        </p>
      </div>
    {/each}
    <p class="text-sm text-gray-400 mt-2">
      This often happens with merged drives (like ZimaOS) or mapped network shares that point to local folders.
    </p>
  </div>
</Modal>

<!-- Search Results Overlay -->
{#if showSearchResults && query.length >= 2}
  <div class="fixed inset-0 bg-black/40 z-40" on:click={closeSearchResults} on:keydown={() => {}} role="button" tabindex="-1"></div>
  <div class="fixed top-32 left-1/2 -translate-x-1/2 w-full max-w-3xl max-h-[60vh] bg-gray-800 rounded-xl shadow-2xl z-50 overflow-hidden border border-gray-700">
    <div class="p-4 border-b border-gray-700 flex items-center justify-between">
      <span class="text-gray-400 text-sm">{formatNumber(searchTotal)} results for "{query}"</span>
      <div class="flex items-center gap-2">
        <button on:click={exportCsv} disabled={searchTotal === 0} class="px-3 py-1 bg-gray-700 hover:bg-gray-600 text-sm text-white rounded-lg transition-colors disabled:opacity-50">Export CSV</button>
        <button on:click={closeSearchResults} class="p-1 text-gray-400 hover:text-white">
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" /></svg>
        </button>
      </div>
    </div>
    <div class="overflow-y-auto max-h-[calc(60vh-60px)]">
      {#each searchResults as file}
        <div class="flex items-center gap-3 p-3 hover:bg-gray-700/50 group transition-colors border-b border-gray-700/50">
          <div class="w-8 h-8 bg-gray-700 rounded flex items-center justify-center flex-shrink-0">
            <svg class="w-4 h-4 {getFileIcon(file.extension)}" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
            </svg>
          </div>
          <div class="flex-1 min-w-0">
            <div class="text-white truncate">{file.name}</div>
            <div class="text-xs text-gray-500 truncate">{file.path}</div>
          </div>
          <div class="text-sm text-gray-400">{formatBytes(file.size)}</div>
          <button on:click={() => openInExplorer(file.path)} class="p-2 text-gray-500 hover:text-white opacity-0 group-hover:opacity-100 transition-all" title="Open in Explorer">
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" />
            </svg>
          </button>
        </div>
      {/each}
    </div>
  </div>
{/if}

<div class="flex flex-col h-screen">
  <!-- Header -->
  <header class="bg-gray-800 border-b border-gray-700 px-6 py-3">
    <div class="flex items-center justify-between">
      <div class="flex items-center gap-4">
        <h1 class="text-xl font-bold text-white">Prism</h1>
        <div class="flex items-center gap-6 text-sm">
          <div>
            <span class="text-gray-400">Files:</span>
            <span class="text-white font-medium ml-1">{formatNumber(totalFiles)}</span>
          </div>
          <div>
            <span class="text-gray-400">Size:</span>
            <span class="text-prism-400 font-medium ml-1">{formatBytes(totalSize)}</span>
          </div>
        </div>
      </div>
      <button on:click={clearDatabase} class="text-sm text-gray-400 hover:text-red-400 transition-colors" title="Clear all data">
        Clear Data
      </button>
    </div>
  </header>

  <!-- Drive Selection with Progress -->
  <div class="bg-gray-800/50 border-b border-gray-700 px-6 py-4">
    {#if loadingDrives}
      <div class="flex items-center gap-2 text-gray-400">
        <div class="w-4 h-4 border-2 border-gray-400 border-t-transparent rounded-full animate-spin"></div>
        <span class="text-sm">Detecting drives...</span>
      </div>
    {:else}
      <div class="flex items-center gap-4">
        <div class="flex items-center gap-3 flex-1 overflow-x-auto pb-1">
          {#each drives as drive}
                {@const scanStatus = getDriveScanStatus(drive.path)}
                {@const indexedFiles = drive.indexed_files || getDriveIndexedCount(drive.path)}
                {@const indexedSize = drive.indexed_size || getDriveIndexedSize(drive.path)}
                {@const driveProgressInfo = driveProgress[drive.path.toUpperCase()]}
                {@const isOffline = !drive.is_online}
                <button
                  on:click={() => isOffline ? null : toggleDrive(drive.path)}
                  disabled={!drive.is_ready || isScanning || isOffline}
                  class="relative flex flex-col gap-1 px-4 py-3 rounded-xl border-2 transition-all min-w-[140px] overflow-hidden
                    {isOffline ? 'opacity-60 border-red-500/50 bg-red-900/10' :
                      scanStatus === 'scanning' ? 'bg-prism-500/10 border-prism-400 ring-2 ring-prism-400/30' :
                      scanStatus === 'done' ? 'bg-green-500/10 border-green-500/50' :
                      selectedDrives.has(drive.path) ? 'bg-prism-500/10 border-prism-500' : 'bg-gray-700/30 border-gray-600 hover:border-gray-500'}
                    {!drive.is_ready && !isOffline ? 'opacity-50 cursor-not-allowed' : ''}"
                >
              <!-- Progress bar fill - same for all drives -->
              {#if scanStatus === 'scanning' || scanStatus === 'done'}
                {@const rawPercent = scanStatus === 'done' ? 100 : (driveProgressInfo?.progress_percent ?? (incrementalProgress?.percent ?? 0))}
                {@const progressPercent = rawPercent >= 0 ? rawPercent : 0}
                <div
                  class="absolute bottom-0 left-0 right-0 transition-all duration-300 ease-out
                    {scanStatus === 'done' ? 'bg-green-500/20' : 'bg-prism-500/30'}"
                  style="height: {progressPercent}%"
                ></div>
                <!-- Animated shimmer overlay while scanning -->
                {#if scanStatus === 'scanning'}
                  <div class="absolute inset-0 overflow-hidden">
                    <div class="absolute inset-0 bg-gradient-to-r from-transparent via-white/5 to-transparent animate-shimmer"></div>
                  </div>
                {/if}
              {/if}

              <div class="relative flex items-center gap-2">
                <!-- Drive letter prominently displayed -->
                <div class="relative flex items-center justify-center w-8 h-8 rounded-lg {isOffline ? 'bg-red-900/30' : scanStatus === 'scanning' ? 'bg-prism-500/30' : 'bg-gray-600/50'}">
                  <span class="text-base font-bold {isOffline ? 'text-red-400' : scanStatus === 'scanning' ? 'text-prism-400' : 'text-white'}">{drive.path.charAt(0)}</span>
                  {#if isOffline}
                    <div class="absolute -top-1 -right-1 w-3 h-3 bg-red-500 rounded-full" title="Offline"></div>
                  {:else if scanStatus === 'scanning'}
                    <div class="absolute -top-1 -right-1 w-3 h-3 bg-prism-500 rounded-full animate-pulse"></div>
                  {:else if scanStatus === 'done' || indexedFiles > 0}
                    <div class="absolute -top-1 -right-1 w-3 h-3 bg-green-500 rounded-full"></div>
                  {/if}
                </div>

                <div class="flex flex-col">
                  <span class="text-sm font-semibold {isOffline ? 'text-red-400' : 'text-white'}">{drive.path.substring(0, 2)}</span>
                  <span class="text-xs {isOffline ? 'text-red-400/70' : 'text-gray-500'}">{isOffline ? 'Offline' : drive.drive_type === 'Local Disk' ? 'Local' : drive.drive_type === 'Network Drive' ? 'Network' : drive.drive_type}</span>
                </div>

                {#if isOffline}
                  <!-- Delete button for offline drives -->
                  <button
                    on:click|stopPropagation={() => removeOfflineDrive(drive.path)}
                    class="w-6 h-6 ml-auto flex items-center justify-center rounded-full bg-red-500/20 hover:bg-red-500/40 text-red-400 hover:text-red-300 transition-colors"
                    title="Remove offline drive data"
                  >
                    <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                    </svg>
                  </button>
                {:else if selectedDrives.has(drive.path) && !isScanning}
                  <svg class="w-4 h-4 text-prism-400 ml-auto" fill="currentColor" viewBox="0 0 20 20">
                    <path fill-rule="evenodd" d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z" clip-rule="evenodd" />
                  </svg>
                {/if}
              </div>

              <!-- Progress/Stats display - same for all drives -->
              <div class="relative text-left">
                {#if scanStatus === 'scanning'}
                  {@const percent = driveProgressInfo?.progress_percent ?? incrementalProgress?.percent ?? 0}
                  {@const filesCount = driveProgressInfo?.files_scanned ?? incrementalProgress?.files_checked ?? 0}
                  <div class="text-xs text-prism-400 font-bold">
                    {#if percent >= 0}
                      Scanning {Math.round(percent)}%
                    {:else}
                      Scanning...
                    {/if}
                  </div>
                  <div class="text-xs text-gray-400">
                    {formatNumber(filesCount)} files
                  </div>
                {:else if scanStatus === 'done'}
                  <div class="text-xs text-green-400 font-bold">Done</div>
                  <div class="text-xs text-gray-400">
                    {formatNumber(driveProgressInfo?.files_scanned ?? indexedFiles)} files
                  </div>
                {:else if isOffline && indexedFiles > 0}
                  <div class="text-xs text-red-400 font-medium">
                    {formatNumber(indexedFiles)} files
                  </div>
                  <div class="text-xs text-red-400/70">
                    {formatBytes(indexedSize)} indexed
                  </div>
                {:else if indexedFiles > 0}
                  <div class="text-xs text-green-400 font-medium">
                    {formatNumber(indexedFiles)} files
                  </div>
                  <div class="text-xs text-gray-500">
                    {formatBytes(indexedSize)}
                  </div>
                {:else if drive.is_ready && drive.total_space > 0}
                  <div class="text-xs text-gray-500">{formatBytes(drive.free_space)} free</div>
                  <div class="text-xs text-gray-600">{formatBytes(drive.total_space)} total</div>
                {:else if isOffline}
                  <div class="text-xs text-red-400/70">Disconnected</div>
                {:else}
                  <div class="text-xs text-gray-500">Not scanned</div>
                {/if}
              </div>
            </button>
          {/each}
        </div>

        <div class="flex items-center gap-2">
          <button on:click={startScan} disabled={selectedDrives.size === 0 || isScanning || checkingOverlaps}
            class="px-5 py-2.5 bg-prism-500 hover:bg-prism-600 text-white rounded-lg transition-colors text-sm font-medium disabled:opacity-50 flex items-center gap-2">
            {#if checkingOverlaps}
              <div class="w-4 h-4 border-2 border-white border-t-transparent rounded-full animate-spin"></div>
              Checking...
            {:else if isScanning}
              <div class="w-4 h-4 border-2 border-white border-t-transparent rounded-full animate-spin"></div>
              Scanning...
            {:else}
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
              </svg>
              Scan
            {/if}
          </button>
        </div>
      </div>

      <!-- Incremental Scan Progress Bar -->
      {#if isScanning && incrementalProgress}
        {@const phase = incrementalProgress.phase}
        {@const percent = incrementalProgress.percent}
        {@const phases = [
          { id: 'preparing', label: 'Prepare', icon: '⚙️' },
          { id: 'scanning', label: 'Scan', icon: '🔍' },
          { id: 'cleaning', label: 'Clean', icon: '🧹' },
          { id: 'indexing', label: 'Index', icon: '📑' },
          { id: 'complete', label: 'Done', icon: '✓' }
        ]}
        {@const phaseIndex = phases.findIndex(p => p.id === phase)}
        <div class="mt-4 bg-gray-900/50 rounded-lg p-3">
          <!-- Phase Indicators -->
          <div class="flex items-center justify-between mb-3">
            <div class="flex items-center gap-1">
              {#each phases as p, i}
                {@const isActive = p.id === phase}
                {@const isPast = i < phaseIndex}
                {@const isFuture = i > phaseIndex}
                <div class="flex items-center">
                  <div class="flex items-center gap-1 px-2 py-1 rounded-full text-xs transition-all
                    {isActive ? 'bg-prism-500/30 text-prism-300 ring-1 ring-prism-400' :
                     isPast ? 'bg-green-500/20 text-green-400' :
                     'bg-gray-700/50 text-gray-500'}">
                    <span>{isPast ? '✓' : p.icon}</span>
                    <span class="hidden sm:inline">{p.label}</span>
                  </div>
                  {#if i < phases.length - 1}
                    <div class="w-2 h-0.5 mx-0.5
                      {isPast ? 'bg-green-500/50' : 'bg-gray-600'}"></div>
                  {/if}
                </div>
              {/each}
            </div>
            <span class="text-sm text-prism-400 font-medium">{percent.toFixed(0)}%</span>
          </div>

          <!-- Progress bar -->
          <div class="w-full bg-gray-700 rounded-full h-2 mb-2">
            <div
              class="bg-gradient-to-r from-prism-500 to-prism-400 h-2 rounded-full transition-all duration-300"
              style="width: {Math.min(percent, 100)}%"
            ></div>
          </div>

          <!-- Stats -->
          <div class="flex flex-wrap gap-4 text-xs text-gray-400">
            <span>Checked: {formatNumber(incrementalProgress.files_checked)}</span>
            {#if incrementalProgress.files_unchanged > 0}
              <span class="text-gray-500">Unchanged: {formatNumber(incrementalProgress.files_unchanged)}</span>
            {/if}
            {#if incrementalProgress.files_new > 0}
              <span class="text-green-400">New: {formatNumber(incrementalProgress.files_new)}</span>
            {/if}
            {#if incrementalProgress.files_updated > 0}
              <span class="text-yellow-400">Updated: {formatNumber(incrementalProgress.files_updated)}</span>
            {/if}
            {#if incrementalProgress.files_deleted > 0}
              <span class="text-red-400">Deleted: {formatNumber(incrementalProgress.files_deleted)}</span>
            {/if}
          </div>
        </div>
      <!-- Overall Scan Progress Bar (Full Scan) -->
      {:else if isScanning && scanProgress}
        {@const isIndexing = scanProgress.indexing != null}
        {@const indexingPercent = scanProgress.indexing?.percent ?? 0}
        {@const indexingRate = scanProgress.indexing?.files_per_second ?? 0}
        {@const totalFilesEstimate = $stats?.totalFiles || 0}
        {@const estimatedPercent = totalFilesEstimate > 0 ? Math.min((scanProgress.files_scanned / totalFilesEstimate) * 100, 100) : 0}
        <div class="mt-4 bg-gray-900/50 rounded-lg p-3">
          <div class="flex items-center justify-between mb-2">
            <div class="flex items-center gap-3">
              <div class="w-4 h-4 border-2 border-prism-400 border-t-transparent rounded-full animate-spin"></div>
              <span class="text-sm text-white font-medium">
                {formatNumber(scanProgress.files_scanned)} files scanned
                {#if totalFilesEstimate > 0}
                  <span class="text-gray-400">/ ~{formatNumber(totalFilesEstimate)}</span>
                {/if}
              </span>
              {#if !isIndexing && scanProgress.files_per_second > 0}
                <span class="text-sm text-gray-400">
                  ({Math.round(scanProgress.files_per_second)} files/sec)
                </span>
              {/if}
            </div>
            <div class="flex items-center gap-2">
              {#if estimatedPercent > 0}
                <span class="text-sm text-prism-400 font-medium">{estimatedPercent.toFixed(0)}%</span>
              {/if}
              <span class="text-sm text-gray-400">{formatBytes(scanProgress.total_size)}</span>
            </div>
          </div>

          <!-- Estimated progress bar (when not indexing) -->
          {#if !isIndexing && estimatedPercent > 0}
            <div class="w-full bg-gray-700 rounded-full h-2 mb-2">
              <div
                class="bg-gradient-to-r from-prism-500 to-prism-400 h-2 rounded-full transition-all duration-300"
                style="width: {estimatedPercent}%"
              ></div>
            </div>
          {/if}

          {#if isIndexing}
            <!-- Indexing progress bar -->
            <div class="mt-3 mb-2">
              <div class="flex items-center justify-between mb-1">
                <span class="text-xs text-gray-400">Building search index...</span>
                <span class="text-xs text-prism-400 font-medium">{indexingPercent.toFixed(0)}%</span>
              </div>
              <div class="w-full bg-gray-700 rounded-full h-2">
                <div
                  class="bg-gradient-to-r from-prism-500 to-prism-400 h-2 rounded-full transition-all duration-300"
                  style="width: {Math.min(indexingPercent, 100)}%"
                ></div>
              </div>
              <div class="flex items-center justify-between mt-1">
                <span class="text-xs text-gray-500">
                  {formatNumber(scanProgress.indexing?.files_indexed ?? 0)} / {formatNumber(scanProgress.indexing?.total_files ?? 0)} files
                </span>
                {#if indexingRate > 0}
                  <span class="text-xs text-gray-500">
                    {formatNumber(Math.round(indexingRate))} files/sec
                  </span>
                {/if}
              </div>
            </div>
          {:else}
            <div class="text-xs text-gray-500 truncate">{scanProgress.current_path}</div>
          {/if}
        </div>
      {/if}
    {/if}
  </div>

  <!-- Search Bar (Always Visible) -->
  <div class="bg-gray-800/30 border-b border-gray-700 px-6 py-3">
    <div class="relative max-w-2xl">
      <svg class="absolute left-3 top-1/2 -translate-y-1/2 w-5 h-5 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
      </svg>
      <input type="text" value={query} on:input={handleSearch} on:focus={() => { if (query.length >= 2) showSearchResults = true; }}
        placeholder="Search files... (size:>1MB ext:pdf type:image path:Documents)"
        class="w-full pl-10 pr-4 py-2.5 bg-gray-800 border border-gray-700 rounded-lg text-white placeholder-gray-500 focus:outline-none focus:border-prism-500" />
      {#if searchLoading}
        <div class="absolute right-3 top-1/2 -translate-y-1/2">
          <div class="w-4 h-4 border-2 border-prism-500 border-t-transparent rounded-full animate-spin"></div>
        </div>
      {/if}
    </div>
  </div>

  <!-- Main Content -->
  <div class="flex-1 flex flex-col overflow-hidden">
    <!-- Tabs -->
    <div class="bg-gray-800/30 border-b border-gray-700 px-6">
      <div class="flex gap-1">
        {#each tabs as tab}
          <button
            on:click={() => switchTab(tab.id)}
            class="px-4 py-3 text-sm font-medium border-b-2 transition-colors {activeTab === tab.id ? 'border-prism-500 text-white' : 'border-transparent text-gray-400 hover:text-white'}"
          >
            {tab.label}
          </button>
        {/each}
      </div>
    </div>

    <!-- Tab Content -->
    <div class="flex-1 overflow-auto p-6">
      <!-- ANALYTICS TAB (Default) -->
      {#if activeTab === 'analytics'}
        <div class="space-y-6">
          {#if analyticsLoading && totalFiles === 0}
            <div class="text-center py-16"><div class="w-8 h-8 border-4 border-prism-500 border-t-transparent rounded-full animate-spin mx-auto"></div></div>
          {:else if totalFiles === 0 && !isScanning}
            <div class="text-center py-16 text-gray-500">
              <svg class="w-16 h-16 mx-auto mb-4 text-gray-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1" d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4m0 5c0 2.21-3.582 4-8 4s-8-1.79-8-4" />
              </svg>
              <p class="text-lg">No files indexed yet</p>
              <p class="text-sm mt-2">Scanning should start automatically...</p>
            </div>
          {:else}
            <!-- Stats Cards -->
            <StatsCards {totalFiles} {totalSize} driveCount={driveStats.length} />

            <!-- Interactive Treemap / Storage Explorer -->
            <StorageExplorer
              {folderContents}
              {driveStats}
              {treemapPath}
              loading={treemapLoading}
              {isScanning}
              on:navigate={(e) => navigateToLevel(e.detail.index)}
              on:drillDown={(e) => drillDownFolder(e.detail)}
            />

            <!-- File Categories with Donut Chart -->
            <FileCategories categories={fileTypeCategories} bind:expandedCategory />

            <!-- Size and Extension Distribution -->
            <SizeDistribution {sizeDistribution} {extensionDistribution} />
          {/if}
        </div>

      <!-- DUPLICATES TAB -->
      {:else if activeTab === 'duplicates'}
        <DuplicateList
          groups={dupGroups}
          loading={dupLoading}
          {wastedSpace}
          {totalFiles}
          expandedGroups={expandedDupGroups}
          selectedFiles={selectedDupFiles}
          on:findDuplicates={findDuplicates}
          on:selectAll={selectAllDuplicates}
          on:deleteSelected={showDeleteConfirm}
          on:toggleGroup={(e) => toggleDupGroup(e.detail.id)}
          on:toggleFile={(e) => toggleDupFile(e.detail.id, e.detail.path)}
          on:openInExplorer={(e) => openInExplorer(e.detail.path)}
        />

      <!-- VERIFY BACKUP TAB -->
      {:else if activeTab === 'verify'}
        <div class="max-w-4xl space-y-6">
          <div class="bg-gray-800/50 rounded-lg p-6">
            <h3 class="text-white font-medium mb-4">Cross-Disk Verification</h3>
            <p class="text-gray-400 text-sm mb-6">
              Verify if all files from a source drive exist on one or more backup drives.
              This helps ensure your backups are complete.
            </p>

            <!-- Source Drive Selection -->
            <div class="mb-4">
              <label class="block text-gray-300 mb-2">Source Drive (to verify)</label>
              <select
                bind:value={verifySourceDrive}
                class="w-full px-3 py-2 bg-gray-700 border border-gray-600 rounded-lg text-white focus:outline-none focus:border-prism-500"
              >
                <option value="">Select source drive...</option>
                {#each driveStats as drive}
                  <option value={drive.path}>{drive.path} - {drive.name} ({formatNumber(drive.file_count)} files)</option>
                {/each}
              </select>
            </div>

            <!-- Target Drives Selection -->
            <div class="mb-6">
              <label class="block text-gray-300 mb-2">Target Drives (backup locations)</label>
              <div class="space-y-2 max-h-48 overflow-y-auto">
                {#each driveStats.filter(d => d.path !== verifySourceDrive) as drive}
                  <label class="flex items-center gap-3 p-2 bg-gray-700/50 rounded hover:bg-gray-700 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={verifyTargetDrives.has(drive.path)}
                      on:change={() => toggleVerifyTarget(drive.path)}
                      class="w-4 h-4 rounded bg-gray-600 border-gray-500 text-prism-500 focus:ring-prism-500"
                    />
                    <span class="text-white">{drive.path}</span>
                    <span class="text-gray-400 text-sm">{drive.name}</span>
                    <span class="text-gray-500 text-xs ml-auto">{formatNumber(drive.file_count)} files</span>
                  </label>
                {/each}
              </div>
            </div>

            <!-- Verify Button -->
            <button
              on:click={runVerification}
              disabled={!verifySourceDrive || verifyTargetDrives.size === 0 || verifyLoading}
              class="w-full py-3 bg-prism-600 hover:bg-prism-500 disabled:bg-gray-600 disabled:cursor-not-allowed text-white font-medium rounded-lg transition-colors"
            >
              {#if verifyLoading}
                <span class="inline-flex items-center gap-2">
                  <span class="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin"></span>
                  {verifyProgress?.message || 'Starting...'}
                </span>
              {:else}
                Verify Backup
              {/if}
            </button>

            <!-- Progress Bar (during verification) -->
            {#if verifyLoading && verifyProgress}
              <div class="mt-4 space-y-2">
                <div class="flex justify-between text-sm text-gray-400">
                  <span>{verifyProgress.message}</span>
                  <span>{verifyProgress.percentage.toFixed(1)}%</span>
                </div>
                <div class="h-2 bg-gray-700 rounded-full overflow-hidden">
                  <div
                    class="h-full bg-prism-500 transition-all duration-300"
                    style="width: {verifyProgress.percentage}%"
                  ></div>
                </div>
                {#if verifyProgress.totalFiles > 0}
                  <div class="text-xs text-gray-500 text-center">
                    {formatNumber(verifyProgress.filesProcessed)} / {formatNumber(verifyProgress.totalFiles)} files
                  </div>
                {/if}
              </div>
            {/if}
          </div>

          <!-- Verification Results -->
          {#if verifyResult}
            <div class="bg-gray-800/50 rounded-lg p-6">
              <h3 class="text-white font-medium mb-4">Verification Results</h3>

              <!-- Summary Stats -->
              <div class="grid grid-cols-2 md:grid-cols-4 gap-4 mb-6">
                <div class="bg-gray-700/50 rounded-lg p-4 text-center">
                  <div class="text-2xl font-bold text-white">{verifyResult.backupPercentage.toFixed(1)}%</div>
                  <div class="text-sm text-gray-400">Backed Up</div>
                </div>
                <div class="bg-gray-700/50 rounded-lg p-4 text-center">
                  <div class="text-2xl font-bold text-green-400">{formatNumber(verifyResult.filesFound)}</div>
                  <div class="text-sm text-gray-400">Files Found</div>
                </div>
                <div class="bg-gray-700/50 rounded-lg p-4 text-center">
                  <div class="text-2xl font-bold text-red-400">{formatNumber(verifyResult.filesMissing)}</div>
                  <div class="text-sm text-gray-400">Missing</div>
                </div>
                <div class="bg-gray-700/50 rounded-lg p-4 text-center">
                  <div class="text-2xl font-bold text-gray-300">{formatBytes(verifyResult.sizeMissing)}</div>
                  <div class="text-sm text-gray-400">Missing Size</div>
                </div>
              </div>

              <!-- Progress Bar -->
              <div class="mb-6">
                <div class="h-4 bg-gray-700 rounded-full overflow-hidden">
                  <div
                    class="h-full transition-all duration-500 {verifyResult.backupPercentage >= 90 ? 'bg-green-500' : verifyResult.backupPercentage >= 70 ? 'bg-yellow-500' : 'bg-red-500'}"
                    style="width: {verifyResult.backupPercentage}%"
                  ></div>
                </div>
              </div>

              <!-- Missing Files List -->
              {#if verifyResult.missingFiles.length > 0}
                <div>
                  <h4 class="text-gray-300 font-medium mb-2">Missing Files (first {verifyResult.missingFiles.length})</h4>
                  <div class="max-h-64 overflow-y-auto space-y-1">
                    {#each verifyResult.missingFiles as file}
                      <div class="flex items-center gap-2 p-2 bg-gray-700/30 rounded text-sm">
                        <span class="text-red-400">✗</span>
                        <span class="text-gray-300 truncate flex-1" title={file.sourcePath}>{file.name}</span>
                        <span class="text-gray-500">{formatBytes(file.size)}</span>
                      </div>
                    {/each}
                  </div>
                </div>
              {:else if verifyResult.filesMissing === 0}
                <div class="text-center py-8 text-green-400">
                  <svg class="w-16 h-16 mx-auto mb-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                  </svg>
                  <p class="text-lg font-medium">All files are backed up!</p>
                </div>
              {/if}

              <div class="mt-4 text-xs text-gray-500">
                Verification completed in {verifyResult.verificationTimeMs}ms
              </div>
            </div>
          {/if}
        </div>

      <!-- SETTINGS TAB -->
      {:else if activeTab === 'settings'}
        <div class="max-w-2xl space-y-6">
          <div class="bg-gray-800/50 rounded-lg p-6">
            <h3 class="text-white font-medium mb-4">Scan Settings</h3>
            <div class="space-y-4">
              <label class="flex items-center justify-between">
                <span class="text-gray-300">Exclude hidden files</span>
                <input type="checkbox" bind:checked={tempSettings.excludeHidden} class="w-5 h-5 rounded bg-gray-700 border-gray-600 text-prism-500 focus:ring-prism-500" />
              </label>
              <label class="flex items-center justify-between">
                <span class="text-gray-300">Exclude system files</span>
                <input type="checkbox" bind:checked={tempSettings.excludeSystem} class="w-5 h-5 rounded bg-gray-700 border-gray-600 text-prism-500 focus:ring-prism-500" />
              </label>
              <label class="block">
                <span class="block text-gray-300 mb-2">Minimum file size (bytes)</span>
                <input type="number" bind:value={tempSettings.minFileSize} min="0" class="w-full px-3 py-2 bg-gray-700 border border-gray-600 rounded-lg text-white focus:outline-none focus:border-prism-500" />
              </label>
            </div>
          </div>

          <div class="bg-gray-800/50 rounded-lg p-6">
            <div class="flex items-center justify-between mb-4">
              <h3 class="text-white font-medium">Exclude Folders</h3>
              <button on:click={addExcludePattern} class="text-sm text-prism-400 hover:text-prism-300">+ Add</button>
            </div>
            <div class="flex flex-wrap gap-2">
              {#each tempSettings.excludePatterns as pattern}
                <span class="px-3 py-1 bg-gray-700 rounded-lg text-sm text-gray-300 flex items-center gap-2">
                  {pattern}
                  <button on:click={() => removeExcludePattern(pattern)} class="text-gray-500 hover:text-red-400">×</button>
                </span>
              {/each}
            </div>
          </div>

          <button on:click={saveSettings} class="px-6 py-2 bg-prism-500 hover:bg-prism-600 text-white rounded-lg">Save Settings</button>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  @keyframes slide-in {
    from { transform: translateX(100%); opacity: 0; }
    to { transform: translateX(0); opacity: 1; }
  }
  .animate-slide-in {
    animation: slide-in 0.3s ease-out;
  }

  @keyframes shimmer {
    0% { transform: translateX(-100%); }
    100% { transform: translateX(100%); }
  }
  .animate-shimmer {
    animation: shimmer 1.5s infinite;
  }
</style>
