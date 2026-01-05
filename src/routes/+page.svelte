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
  import TreeExplorer from '$lib/components/analytics/TreeExplorer.svelte';
  import FileCategories from '$lib/components/analytics/FileCategories.svelte';
  import SizeDistribution from '$lib/components/analytics/SizeDistribution.svelte';
  import DuplicateList from '$lib/components/duplicates/DuplicateList.svelte';

  // Types
  import type { DriveStatus, DriveProgress, ScanProgress as ScanProgressType, DriveStats, TabType, PreScanOverlapResult } from '$lib/types';

  // Use the centralized notification store
  function notify(message: string, type: 'success' | 'error' | 'info' = 'info') {
    notificationStore.notify(message, type);
  }

  // Drives
  let drives: DriveStatus[] = [];
  let selectedDrives = new Set<string>();
  let loadingDrives = true;
  let driveStats: DriveStats[] = [];

  // Per-drive scan progress tracking (from backend)
  let driveProgress: Record<string, DriveProgress> = {};

  // Scan state
  let isScanning = false;
  let scanProgress: ScanProgressType | null = null;
  let unlistenProgress: (() => void) | null = null;
  let unlistenComplete: (() => void) | null = null;
  let unlistenStats: (() => void) | null = null;

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

      // Update per-drive progress from backend
      if (scanProgress?.drives) {
        driveProgress = scanProgress.drives;
      }

      if (scanProgress?.is_complete) {
        isScanning = false;

        if (scanProgress.error) {
          notify(`Scan failed: ${scanProgress.error}`, 'error');
        } else {
          notify(`Scan complete: ${formatNumber(scanProgress.files_scanned)} files indexed`, 'success');
          loadAnalytics();
          loadDriveStats();
        }
      }
    });

    unlistenComplete = await listen('scan-complete', async () => {
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

    // ALWAYS auto-scan on startup
    if (drives.some(d => d.is_ready && d.drive_type === 'Local Disk')) {
      notify('Starting automatic scan of all drives...', 'info');
      await startAutoScan();
    }
  });

  onDestroy(() => {
    unlistenProgress?.();
    unlistenComplete?.();
    unlistenStats?.();
  });

  async function loadDrives() {
    loadingDrives = true;
    try {
      drives = await invoke<DriveStatus[]>('get_drives_status');
      // Auto-select online local disks that haven't been scanned yet
      drives.forEach(d => {
        if (d.is_online && d.drive_type === 'Local Disk' && d.is_ready) {
          selectedDrives.add(d.path);
        }
      });
      selectedDrives = selectedDrives;
    } catch (e) {
      console.error('Failed to load drives:', e);
      notify('Failed to detect drives', 'error');
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
    driveProgress = {};
    scanProgress = { scan_id: 0, files_scanned: 0, total_size: 0, current_path: 'Initializing...', files_per_second: 0, is_complete: false, error: null, drives: {} };

    try {
      await invoke('auto_scan_drives');
    } catch (e) {
      notify(`Auto-scan failed: ${e}`, 'error');
      isScanning = false;
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
  function getDriveScanStatus(drivePath: string): 'idle' | 'waiting' | 'scanning' | 'done' {
    if (!isScanning) return 'idle';
    const normalizedPath = drivePath.toUpperCase();
    const progress = driveProgress[normalizedPath];
    if (progress) {
      return progress.status as 'waiting' | 'scanning' | 'done';
    }
    return 'waiting';
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
    { id: 'settings', label: 'Settings' }
  ];

  // Analytics view mode (treemap or tree)
  let analyticsViewMode: 'treemap' | 'tree' = 'tree';

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
                  selectedDrives.has(drive.path) ? 'bg-prism-500/10 border-prism-500' : 'bg-gray-700/30 border-gray-600 hover:border-gray-500'}
                {!drive.is_ready && !isOffline ? 'opacity-50 cursor-not-allowed' : ''}
                {scanStatus === 'scanning' ? 'border-prism-400 ring-2 ring-prism-400/30' : ''}
                {scanStatus === 'done' ? 'border-green-500/50' : ''}"
            >
              <!-- Progress bar fill - fills from bottom to top -->
              {#if (scanStatus === 'scanning' || scanStatus === 'done') && driveProgressInfo}
                <div
                  class="absolute bottom-0 left-0 right-0 transition-all duration-300 ease-out
                    {scanStatus === 'done' ? 'bg-green-500/20' : 'bg-prism-500/30'}"
                  style="height: {driveProgressInfo.progress_percent}%"
                ></div>
                <!-- Animated shimmer overlay while scanning -->
                {#if scanStatus === 'scanning'}
                  <div class="absolute inset-0 overflow-hidden">
                    <div class="absolute inset-0 bg-gradient-to-r from-transparent via-white/5 to-transparent animate-shimmer"></div>
                  </div>
                {/if}
              {:else if scanStatus === 'waiting'}
                <div class="absolute inset-0 bg-yellow-500/10"></div>
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
                  {:else if scanStatus === 'waiting'}
                    <div class="absolute -top-1 -right-1 w-3 h-3 bg-yellow-500 rounded-full"></div>
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

              <!-- Progress/Stats display -->
              <div class="relative text-left">
                {#if scanStatus === 'scanning' && driveProgressInfo}
                  <div class="text-xs text-prism-400 font-bold">
                    Scanning {Math.round(driveProgressInfo.progress_percent)}%
                  </div>
                  <div class="text-xs text-gray-400">
                    {formatNumber(driveProgressInfo.files_scanned)} files
                  </div>
                  <div class="text-xs text-gray-500">
                    {formatBytes(driveProgressInfo.total_size)}
                  </div>
                {:else if scanStatus === 'done' && driveProgressInfo}
                  <div class="text-xs text-green-400 font-bold">
                    Scanned 100%
                  </div>
                  <div class="text-xs text-gray-400">
                    {formatNumber(driveProgressInfo.files_scanned)} files
                  </div>
                  <div class="text-xs text-gray-500">
                    {formatBytes(driveProgressInfo.total_size)}
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
                {:else if scanStatus === 'waiting'}
                  <div class="text-xs text-yellow-400">Waiting...</div>
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
          <button on:click={loadDrives} disabled={isScanning} class="p-2 text-gray-400 hover:text-white transition-colors" title="Refresh drives">
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
            </svg>
          </button>
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

      <!-- Overall Scan Progress Bar -->
      {#if isScanning && scanProgress}
        {@const isIndexing = scanProgress.indexing != null}
        {@const indexingPercent = scanProgress.indexing?.percent ?? 0}
        {@const indexingRate = scanProgress.indexing?.files_per_second ?? 0}
        <div class="mt-4 bg-gray-900/50 rounded-lg p-3">
          <div class="flex items-center justify-between mb-2">
            <div class="flex items-center gap-3">
              <div class="w-4 h-4 border-2 border-prism-400 border-t-transparent rounded-full animate-spin"></div>
              <span class="text-sm text-white font-medium">
                {formatNumber(scanProgress.files_scanned)} files scanned
              </span>
              {#if !isIndexing && scanProgress.files_per_second > 0}
                <span class="text-sm text-gray-400">
                  ({Math.round(scanProgress.files_per_second)} files/sec)
                </span>
              {/if}
            </div>
            <span class="text-sm text-prism-400 font-medium">{formatBytes(scanProgress.total_size)}</span>
          </div>

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

            <!-- View Mode Toggle -->
            <div class="flex items-center gap-2 mb-4">
              <span class="text-sm text-gray-400">View:</span>
              <div class="flex bg-gray-800 rounded-lg p-1">
                <button
                  class="px-3 py-1.5 text-sm rounded-md transition-colors {analyticsViewMode === 'tree' ? 'bg-prism-600 text-white' : 'text-gray-400 hover:text-white'}"
                  on:click={() => analyticsViewMode = 'tree'}
                >
                  Tree
                </button>
                <button
                  class="px-3 py-1.5 text-sm rounded-md transition-colors {analyticsViewMode === 'treemap' ? 'bg-prism-600 text-white' : 'text-gray-400 hover:text-white'}"
                  on:click={() => analyticsViewMode = 'treemap'}
                >
                  Treemap
                </button>
              </div>
            </div>

            <!-- Storage Explorer (Tree or Treemap based on view mode) -->
            {#if analyticsViewMode === 'tree'}
              <TreeExplorer maxDepth={4} />
            {:else}
              <StorageExplorer
                {folderContents}
                {driveStats}
                {treemapPath}
                loading={treemapLoading}
                {isScanning}
                on:navigate={(e) => navigateToLevel(e.detail.index)}
                on:drillDown={(e) => drillDownFolder(e.detail)}
              />
            {/if}

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
