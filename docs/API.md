# Tauri API Reference

All commands are invoked from the frontend using `invoke()`:

```typescript
import { invoke } from '@tauri-apps/api/core';
const result = await invoke<ReturnType>('command_name', { param1, param2 });
```

## Scanning Commands

### `start_scan`

Start scanning specified directories.

```typescript
interface ScanRequest {
    paths: string[];
    excludeHidden?: boolean;
    excludeSystem?: boolean;
    excludePatterns?: string[];
    minFileSize?: number;
}

interface ScanProgress {
    scan_id: number;
    files_scanned: number;
    total_size: number;
    current_path: string;
    files_per_second: number;
    is_complete: boolean;
    error: string | null;
    drives: Record<string, DriveProgress>;
}

await invoke<ScanProgress>('start_scan', { request: ScanRequest });
```

### `auto_scan_drives`

Automatically detect and scan all local drives.

```typescript
await invoke('auto_scan_drives');
```

### `get_available_drives`

Get list of available drives.

```typescript
interface DriveInfo {
    path: string;
    name: string;
    drive_type: string;
    total_space: number;
    free_space: number;
    is_ready: boolean;
}

await invoke<DriveInfo[]>('get_available_drives');
```

### `get_drive_stats`

Get statistics for indexed drives.

```typescript
interface DriveStats {
    path: string;
    name: string;
    file_count: number;
    total_size: number;
    extensions: { extension: string; count: number; size: number }[];
}

await invoke<DriveStats[]>('get_drive_stats');
```

## Search Commands

### `quick_search`

Fast FTS5-powered search with filters.

```typescript
interface QuickSearchResponse {
    results: SearchResult[];
    total: number;
}

interface SearchResult {
    id: number;
    path: string;
    name: string;
    extension: string | null;
    size: number;
    modified_at: number | null;
}

await invoke<QuickSearchResponse>('quick_search', {
    query: 'size:>1MB ext:pdf',
    limit: 100
});
```

Supported filters:
- `size:>1MB`, `size:<100KB`, `size:1GB-5GB`
- `ext:pdf,docx,xlsx`
- `type:image`, `type:video`, `type:audio`, `type:document`, `type:archive`
- `path:Documents`

### `search_files_advanced`

Advanced search with pagination and sorting.

```typescript
interface AdvancedSearchRequest {
    query: string;
    limit?: number;
    offset?: number;
    sort_by?: string;
    sort_order?: string;
}

interface AdvancedSearchResponse {
    results: SearchResult[];
    total_count: number;
    query_time_ms: number;
}

await invoke<AdvancedSearchResponse>('search_files_advanced', { request });
```

## Duplicate Commands

### `find_duplicates`

Find exact duplicate files by hash.

```typescript
interface DuplicateGroup {
    id: number;
    hash: string;
    size: number;
    wasted_space: number;
    files: DuplicateFile[];
}

interface DuplicateFile {
    id: number;
    path: string;
    name: string;
    modified_at: number | null;
}

await invoke<DuplicateGroup[]>('find_duplicates');
```

### `find_similar_images`

Find visually similar images using perceptual hashing.

```typescript
interface SimilarImagesResponse {
    groups: SimilarImageGroup[];
    total_groups: number;
    total_images: number;
    detection_time_ms: number;
}

interface SimilarImageGroup {
    id: number;
    images: SimilarImage[];
    similarity_threshold: number;
}

await invoke<SimilarImagesResponse>('find_similar_images', {
    threshold: 10  // Hamming distance, lower = more similar
});
```

### `delete_duplicate`

Delete a single duplicate file.

```typescript
await invoke<boolean>('delete_duplicate', {
    fileId: 123,
    path: 'C:\\path\\to\\file.txt'
});
```

### `delete_duplicates_batch`

Delete multiple duplicate files.

```typescript
// Array of [id, path] tuples
await invoke<number>('delete_duplicates_batch', {
    files: [[123, 'C:\\file1.txt'], [456, 'C:\\file2.txt']]
});
```

## Analytics Commands

### `get_stats`

Get overall application statistics.

```typescript
interface AppStats {
    total_files: number;
    total_size: number;
    duplicate_count: number;
    last_scan: ScanInfo | null;
}

await invoke<AppStats>('get_stats');
```

### `get_size_distribution`

Get file count by size category.

```typescript
interface SizeCategory {
    name: string;       // "< 1KB", "1KB - 1MB", etc.
    count: number;
    total_size: number;
}

await invoke<SizeCategory[]>('get_size_distribution');
```

### `get_extension_distribution`

Get top extensions by size.

```typescript
interface ExtensionCategory {
    extension: string;
    count: number;
    total_size: number;
}

await invoke<ExtensionCategory[]>('get_extension_distribution', { limit: 20 });
```

### `get_file_type_distribution`

Get files grouped by type category.

```typescript
interface FileTypeCategory {
    category: string;   // "Documents", "Images", "Videos", etc.
    count: number;
    total_size: number;
    color: string;      // Hex color for UI
    extensions: ExtensionDetail[];
}

interface ExtensionDetail {
    extension: string;
    count: number;
    size: number;
}

await invoke<FileTypeCategory[]>('get_file_type_distribution');
```

### `get_folder_contents`

Get folder contents for treemap navigation.

```typescript
interface FolderItem {
    path: string;
    name: string;
    size: number;
    file_count: number;
    is_folder: boolean;
}

// null path = root (all drives)
await invoke<FolderItem[]>('get_folder_contents', {
    path: 'C:\\Users',
    limit: 20
});
```

### `get_folder_sizes`

Get folder sizes for analytics.

```typescript
interface FolderSize {
    path: string;
    name: string;
    size: number;
    depth: number;
}

await invoke<FolderSize[]>('get_folder_sizes', {
    rootPath: 'C:\\',
    depth: 2
});
```

### `get_treemap_data`

Get pre-computed treemap data for fast visualization.

```typescript
interface TreemapRequest {
    drive: string | null;    // null = all drives
    path: string | null;     // null = root
    max_depth: number;       // Tree depth to fetch
    min_size: number;        // Minimum folder size (bytes)
}

interface TreemapNode {
    path: string;
    name: string;
    size: number;
    file_count: number;
    children: TreemapNode[];
}

await invoke<TreemapNode[]>('get_treemap_data', {
    request: {
        drive: 'C:\\',
        path: null,
        max_depth: 4,
        min_size: 10485760  // 10MB
    }
});
```

### `get_folder_children`

Get direct children of a folder with size info.

```typescript
interface FolderSize {
    id: number;
    path: string;
    name: string;
    drive: string;
    depth: number;
    total_size: number;
    file_count: number;
    folder_count: number;
    parent_path: string | null;
}

await invoke<FolderSize[]>('get_folder_children', {
    path: 'C:\\Users',
    minSize: 1048576,  // 1MB
    limit: 100
});
```

### `rebuild_folder_sizes`

Manually trigger folder sizes rebuild.

```typescript
// Returns number of folders processed
await invoke<number>('rebuild_folder_sizes');
```

## Utility Commands

### `open_in_explorer`

Open file or folder in Windows Explorer.

```typescript
await invoke('open_in_explorer', { path: 'C:\\path\\to\\file.txt' });
```

### `export_to_csv`

Export search results to CSV.

```typescript
await invoke<number>('export_to_csv', {
    outputPath: 'C:\\export.csv',
    query: 'ext:pdf'
});
```

### `export_to_json`

Export search results to JSON.

```typescript
await invoke<number>('export_to_json', {
    outputPath: 'C:\\export.json',
    query: 'ext:pdf'
});
```

### `clear_database`

Clear all scanned data.

```typescript
await invoke('clear_database');
```

### `close_search_window`

Close the quick search overlay.

```typescript
await invoke('close_search_window');
```

## Events

The backend emits events during long-running operations:

### `scan-progress`

Emitted during scanning.

```typescript
import { listen } from '@tauri-apps/api/event';

await listen<ScanProgress>('scan-progress', (event) => {
    console.log('Files scanned:', event.payload.files_scanned);
});
```

### `scan-complete`

Emitted when scan finishes.

```typescript
await listen('scan-complete', () => {
    console.log('Scan finished!');
});
```

### `stats-updated`

Emitted when statistics change.

```typescript
await listen('stats-updated', () => {
    // Reload stats
});
```

### `incremental-progress`

Emitted during incremental scans with detailed phase info.

```typescript
interface IncrementalProgress {
    phase: 'preparing' | 'scanning' | 'cleaning' | 'indexing' | 'complete';
    percent: number;
    files_checked: number;
    files_new: number;
    files_updated: number;
    files_deleted: number;
    files_unchanged: number;
    current_drive: string | null;
}

await listen<IncrementalProgress>('incremental-progress', (event) => {
    console.log('Phase:', event.payload.phase);
});
```

### `auto-scan-phase`

Emitted when auto-scan changes phase.

```typescript
interface AutoScanPhase {
    phase: string;       // 'local' | 'network' | 'all'
    has_network: boolean;
    paths: string[];
}

await listen<AutoScanPhase>('auto-scan-phase', (event) => {
    console.log('Scanning:', event.payload.phase);
});
```

### `auto-scan-complete`

Emitted when auto-scan finishes all drives.

```typescript
await listen('auto-scan-complete', () => {
    console.log('All drives scanned!');
});
```

### `treemap-ready`

Emitted when folder sizes have been rebuilt.

```typescript
await listen('treemap-ready', () => {
    // Reload treemap data
});
```
