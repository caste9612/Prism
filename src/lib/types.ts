// Shared types for Prism application
// All types use snake_case to match Rust backend serialization

// ============================================================================
// Drive Types
// ============================================================================

export interface DriveInfo {
  path: string;
  name: string;
  drive_type: string;
  total_space: number;
  free_space: number;
  is_ready: boolean;
}

export type DriveOnlineStatus = 'online' | 'offline' | 'ready_to_sync' | 'never_scanned';

export interface DriveStatus extends DriveInfo {
  is_online: boolean;
  is_scanned: boolean;
  last_scan_at: number | null;
  indexed_files: number;
  indexed_size: number;
  status: DriveOnlineStatus;
}

export interface DriveProgress {
  drive: string;
  files_scanned: number;
  total_size: number;
  status: 'waiting' | 'scanning' | 'done';
  progress_percent: number;
  estimated_total: number;
}

export interface IndexingProgress {
  files_indexed: number;
  total_files: number;
  percent: number;
  files_per_second: number;
}

export interface IncrementalProgress {
  phase: 'preparing' | 'scanning' | 'cleaning' | 'indexing' | 'complete';
  files_checked: number;
  files_unchanged: number;
  files_updated: number;
  files_new: number;
  files_deleted: number;
  total_files_in_db: number;
  percent: number;
}

export interface ScanProgress {
  scan_id: number;
  files_scanned: number;
  total_size: number;
  current_path: string;
  files_per_second: number;
  is_complete: boolean;
  error: string | null;
  drives: Record<string, DriveProgress>;
  indexing?: IndexingProgress;
}

export interface DriveStats {
  path: string;
  name: string;
  file_count: number;
  total_size: number;
  extensions: { extension: string; count: number; size: number }[];
}

export interface DriveOverlap {
  source_drive: string;
  target_drive: string;
  overlap_percent: number;
  sample_size: number;
  matches_found: number;
}

export interface PreScanOverlapResult {
  drive_path: string;
  is_overlapping: boolean;
  overlaps_with: string[];
  overlap_percent: number;
  sample_size: number;
  recommendation: 'scan' | 'skip' | 'warn';
}

export interface Notification {
  id: number;
  message: string;
  type: 'success' | 'error' | 'info';
}

export interface FileResult {
  id: number;
  path: string;
  name: string;
  extension: string | null;
  size: number;
  modified_at: number | null;
}

export interface DuplicateFile {
  id: number;
  path: string;
  name: string;
  extension: string | null;
  modified_at: number | null;
  is_original: boolean;
}

export interface DuplicateGroup {
  id: number;
  hash: string;
  size: number;
  wasted_space: number;
  files: DuplicateFile[];
}

export interface FolderItem {
  path: string;
  name: string;
  size: number;
  file_count: number;
  is_folder: boolean;
}

export interface FileTypeCategory {
  category: string;
  count: number;
  total_size: number;
  color: string;
  extensions: { extension: string; count: number; size: number }[];
}

export interface SizeCategory {
  name: string;
  count: number;
  total_size: number;
}

export interface ExtensionCategory {
  extension: string;
  count: number;
  total_size: number;
}

export type TabType = 'analytics' | 'duplicates' | 'verify' | 'settings';

// ============================================================================
// Search Types
// ============================================================================

export interface SearchResult {
  id: number;
  path: string;
  name: string;
  extension: string | null;
  size: number;
  modified_at: number | null;
  rank?: number;
  snippet?: string | null;
}

export interface SearchResponse {
  results: SearchResult[];
  total_count: number;
  query_time_ms: number;
}

// ============================================================================
// Duplicate Types
// ============================================================================

export interface DuplicateResponse {
  groups: DuplicateGroup[];
  total_groups: number;
  total_wasted_space: number;
  detection_time_ms: number;
}

export interface SimilarImage {
  id: number;
  path: string;
  name: string;
  size: number;
  phash: string;
  distance_from_first: number;
}

export interface SimilarImageGroup {
  id: number;
  images: SimilarImage[];
  similarity_threshold: number;
}

export interface SimilarImagesResponse {
  groups: SimilarImageGroup[];
  total_groups: number;
  total_images: number;
  detection_time_ms: number;
}

// ============================================================================
// Analytics Types
// ============================================================================

export interface FolderSize {
  path: string;
  name: string;
  size: number;
  depth: number;
}

export interface ExtensionDetail {
  extension: string;
  count: number;
  size: number;
}

// ============================================================================
// App State Types
// ============================================================================

export interface AppStats {
  total_files: number;
  total_size: number;
  duplicate_count: number;
  last_scan: ScanInfo | null;
}

export interface ScanInfo {
  id: number;
  started_at: number;
  completed_at: number | null;
  root_paths: string;
  files_scanned: number;
  total_size: number;
  status: string;
}

// ============================================================================
// Tree View Types
// ============================================================================

export type NodeType = 'drive' | 'folder' | 'file';

export interface TreeNode {
  path: string;
  name: string;
  size: number;
  fileCount: number;
  nodeType: NodeType;
  depth: number;
  children: TreeNode[];
}

// ============================================================================
// Cross-Disk Verification Types
// ============================================================================

export interface FileVerificationResult {
  sourcePath: string;
  name: string;
  size: number;
  found: boolean;
  foundOn: string[];
  matchedPath: string | null;
}

export interface VerificationSummary {
  sourceDrive: string;
  targetDrives: string[];
  totalFiles: number;
  filesFound: number;
  filesMissing: number;
  backupPercentage: number;
  totalSize: number;
  sizeFound: number;
  sizeMissing: number;
  missingFiles: FileVerificationResult[];
  verificationTimeMs: number;
}

// ============================================================================
// Settings Types
// ============================================================================

export interface Settings {
  excludeHidden: boolean;
  excludeSystem: boolean;
  minFileSize: number;
  excludePatterns: string[];
  theme: 'dark' | 'light' | 'system';
}
