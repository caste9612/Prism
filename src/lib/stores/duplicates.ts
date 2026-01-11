import { writable, get, derived } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { DuplicateFile, DuplicateGroup, DuplicateResponse, SimilarImageGroup, SimilarImagesResponse } from '$lib/types';

// Types
export type DuplicateTab = 'exact' | 'similar' | 'byFolder';
export type SortField = 'wastedSpace' | 'fileCount' | 'size' | 'name';
export type SortOrder = 'asc' | 'desc';

export interface DuplicateProgress {
  phase: 'candidates' | 'hashing' | 'grouping' | 'complete';
  percent: number;
  candidateGroups: number;
  confirmedGroups: number;
  filesProcessed: number;
  totalFiles: number;
  message: string;
}

export interface FolderDuplicates {
  path: string;
  name: string;
  duplicateCount: number;
  wastedSpace: number;
  groups: DuplicateGroup[];
}

interface DuplicatesState {
  // Core data
  groups: DuplicateGroup[];
  similarGroups: SimilarImageGroup[];

  // Stats
  totalGroups: number;
  totalWastedSpace: number;
  detectionTimeMs: number;
  totalSimilarGroups: number;
  totalSimilarImages: number;
  similarDetectionTimeMs: number;

  // UI State
  activeTab: DuplicateTab;
  loading: boolean;
  similarLoading: boolean;
  error: string | null;
  progress: DuplicateProgress | null;

  // Selection
  selectedFiles: Set<string>; // Changed to "id:path" format for safety

  // Filters
  sortBy: SortField;
  sortOrder: SortOrder;
  filterExtensions: string[];
  filterMinSize: number;
  searchQuery: string;
}

interface BatchDeleteResponse {
  deleted: number;
  errors: string[];
}

// Calculate "Original" score - higher = more likely to be the original
export function calculateOriginalScore(file: DuplicateFile): number {
  let score = 0;

  // Prefer shorter paths (less nested = more "main")
  const pathDepth = file.path.split(/[\\\/]/).length;
  score += 100 - Math.min(pathDepth * 5, 50);

  // Penalize backup/archive folders
  const pathLower = file.path.toLowerCase();
  if (pathLower.includes('backup')) score -= 20;
  if (pathLower.includes('archive')) score -= 15;
  if (pathLower.includes('copy')) score -= 10;
  if (pathLower.includes('old')) score -= 10;
  if (pathLower.includes('temp')) score -= 25;
  if (/\(\d+\)/.test(file.name)) score -= 25; // file (1).txt pattern
  if (/ - copy/i.test(file.name)) score -= 20;

  // Prefer local drives (C:, D:, E:) over network/removable
  if (/^[A-E]:/i.test(file.path)) score += 10;

  // Prefer oldest modification time
  if (file.modified_at) {
    // Normalize to a score contribution (older = higher score)
    const ageBonus = Math.min((Date.now() / 1000 - file.modified_at) / 86400000, 30); // up to 30 points
    score += ageBonus;
  }

  return score;
}

// Sort files within a group so the "best" original is first
export function sortGroupFiles(files: DuplicateFile[]): DuplicateFile[] {
  return [...files].sort((a, b) => calculateOriginalScore(b) - calculateOriginalScore(a));
}

// Extract unique extensions from groups
function getUniqueExtensions(groups: DuplicateGroup[]): string[] {
  const extensions = new Set<string>();
  for (const group of groups) {
    for (const file of group.files) {
      if (file.extension) {
        extensions.add(file.extension.toLowerCase());
      }
    }
  }
  return Array.from(extensions).sort();
}

// Group duplicates by folder
function groupByFolder(groups: DuplicateGroup[]): FolderDuplicates[] {
  const folderMap = new Map<string, FolderDuplicates>();

  for (const group of groups) {
    for (const file of group.files) {
      const lastSep = Math.max(file.path.lastIndexOf('\\'), file.path.lastIndexOf('/'));
      const folderPath = lastSep > 0 ? file.path.substring(0, lastSep) : file.path;
      const folderName = folderPath.split(/[\\\/]/).pop() || folderPath;

      if (!folderMap.has(folderPath)) {
        folderMap.set(folderPath, {
          path: folderPath,
          name: folderName,
          duplicateCount: 0,
          wastedSpace: 0,
          groups: []
        });
      }

      const folder = folderMap.get(folderPath)!;
      if (!folder.groups.includes(group)) {
        folder.groups.push(group);
        // Only count non-original files as duplicates
        const nonOriginals = group.files.filter(f => !f.is_original).length;
        folder.duplicateCount += nonOriginals;
        folder.wastedSpace += group.wasted_space;
      }
    }
  }

  return Array.from(folderMap.values())
    .filter(f => f.duplicateCount > 0)
    .sort((a, b) => b.wastedSpace - a.wastedSpace);
}

function createDuplicatesStore() {
  const store = writable<DuplicatesState>({
    groups: [],
    similarGroups: [],
    totalGroups: 0,
    totalWastedSpace: 0,
    detectionTimeMs: 0,
    totalSimilarGroups: 0,
    totalSimilarImages: 0,
    similarDetectionTimeMs: 0,
    activeTab: 'exact',
    loading: false,
    similarLoading: false,
    error: null,
    progress: null,
    selectedFiles: new Set(),
    sortBy: 'wastedSpace',
    sortOrder: 'desc',
    filterExtensions: [],
    filterMinSize: 0,
    searchQuery: ''
  });

  const { subscribe, set, update } = store;

  // Derived stores for computed values
  const filteredGroups = derived(store, ($state) => {
    let groups = [...$state.groups];

    // Apply extension filter
    if ($state.filterExtensions.length > 0) {
      groups = groups.filter(group =>
        group.files.some(f =>
          f.extension && $state.filterExtensions.includes(f.extension.toLowerCase())
        )
      );
    }

    // Apply min size filter
    if ($state.filterMinSize > 0) {
      groups = groups.filter(group => group.size >= $state.filterMinSize);
    }

    // Apply search query
    if ($state.searchQuery.trim()) {
      const query = $state.searchQuery.toLowerCase();
      groups = groups.filter(group =>
        group.files.some(f =>
          f.name.toLowerCase().includes(query) ||
          f.path.toLowerCase().includes(query)
        )
      );
    }

    // Sort groups with better original detection
    groups = groups.map(group => ({
      ...group,
      files: sortGroupFiles(group.files).map((f, idx) => ({
        ...f,
        is_original: idx === 0 // First after sorting is the "original"
      }))
    }));

    // Sort groups
    groups.sort((a, b) => {
      let cmp = 0;
      switch ($state.sortBy) {
        case 'wastedSpace':
          cmp = b.wasted_space - a.wasted_space;
          break;
        case 'fileCount':
          cmp = b.files.length - a.files.length;
          break;
        case 'size':
          cmp = b.size - a.size;
          break;
        case 'name':
          cmp = (a.files[0]?.name || '').localeCompare(b.files[0]?.name || '');
          break;
      }
      return $state.sortOrder === 'desc' ? cmp : -cmp;
    });

    return groups;
  });

  const availableExtensions = derived(store, ($state) => getUniqueExtensions($state.groups));
  const folderDuplicates = derived(store, ($state) => groupByFolder($state.groups));

  const getState = () => get(store);

  // Progress listener cleanup function
  let unlistenProgress: (() => void) | null = null;

  return {
    subscribe,
    filteredGroups,
    availableExtensions,
    folderDuplicates,

    // Tab management
    setActiveTab: (tab: DuplicateTab) => {
      update(state => ({ ...state, activeTab: tab }));
    },

    // Filter management
    setSortBy: (field: SortField) => {
      update(state => ({ ...state, sortBy: field }));
    },

    setSortOrder: (order: SortOrder) => {
      update(state => ({ ...state, sortOrder: order }));
    },

    toggleSortOrder: () => {
      update(state => ({
        ...state,
        sortOrder: state.sortOrder === 'asc' ? 'desc' : 'asc'
      }));
    },

    setFilterExtensions: (extensions: string[]) => {
      update(state => ({ ...state, filterExtensions: extensions }));
    },

    toggleExtensionFilter: (ext: string) => {
      update(state => {
        const newFilters = state.filterExtensions.includes(ext)
          ? state.filterExtensions.filter(e => e !== ext)
          : [...state.filterExtensions, ext];
        return { ...state, filterExtensions: newFilters };
      });
    },

    setFilterMinSize: (size: number) => {
      update(state => ({ ...state, filterMinSize: size }));
    },

    setSearchQuery: (query: string) => {
      update(state => ({ ...state, searchQuery: query }));
    },

    clearFilters: () => {
      update(state => ({
        ...state,
        filterExtensions: [],
        filterMinSize: 0,
        searchQuery: ''
      }));
    },

    // Find exact duplicates
    findDuplicates: async (minSize?: number) => {
      update(state => ({
        ...state,
        loading: true,
        error: null,
        progress: {
          phase: 'candidates',
          percent: 0,
          candidateGroups: 0,
          confirmedGroups: 0,
          filesProcessed: 0,
          totalFiles: 0,
          message: 'Finding candidate duplicates...'
        }
      }));

      // Set up progress listener
      try {
        unlistenProgress = await listen<DuplicateProgress>('duplicate-progress', (event) => {
          update(state => ({ ...state, progress: event.payload }));
        });
      } catch (e) {
        console.warn('Could not set up progress listener:', e);
      }

      try {
        const response = await invoke<DuplicateResponse>('find_duplicates', {
          minSize: minSize ?? null,
        });

        update(state => ({
          ...state,
          groups: response.groups,
          totalGroups: response.total_groups,
          totalWastedSpace: response.total_wasted_space,
          detectionTimeMs: response.detection_time_ms,
          loading: false,
          progress: null,
          selectedFiles: new Set(),
        }));

        return response;
      } catch (e) {
        console.error('Duplicate detection failed:', e);
        update(state => ({
          ...state,
          loading: false,
          progress: null,
          error: String(e),
        }));
        throw e;
      } finally {
        if (unlistenProgress) {
          unlistenProgress();
          unlistenProgress = null;
        }
      }
    },

    // Find similar images
    findSimilarImages: async (threshold?: number) => {
      update(state => ({
        ...state,
        similarLoading: true,
        error: null,
        progress: {
          phase: 'hashing',
          percent: 0,
          candidateGroups: 0,
          confirmedGroups: 0,
          filesProcessed: 0,
          totalFiles: 0,
          message: 'Computing perceptual hashes...'
        }
      }));

      try {
        const response = await invoke<SimilarImagesResponse>('find_similar_images', {
          threshold: threshold ?? 10,
        });

        update(state => ({
          ...state,
          similarGroups: response.groups,
          totalSimilarGroups: response.total_groups,
          totalSimilarImages: response.total_images,
          similarDetectionTimeMs: response.detection_time_ms,
          similarLoading: false,
          progress: null,
        }));

        return response;
      } catch (e) {
        console.error('Similar image detection failed:', e);
        update(state => ({
          ...state,
          similarLoading: false,
          progress: null,
          error: String(e),
        }));
        throw e;
      }
    },

    // Selection management with safe key format
    toggleFileSelection: (fileId: number, filePath: string) => {
      const key = `${fileId}:${filePath}`;
      update(state => {
        const newSelected = new Set(state.selectedFiles);
        if (newSelected.has(key)) {
          newSelected.delete(key);
        } else {
          newSelected.add(key);
        }
        return { ...state, selectedFiles: newSelected };
      });
    },

    isFileSelected: (fileId: number, filePath: string): boolean => {
      const state = getState();
      return state.selectedFiles.has(`${fileId}:${filePath}`);
    },

    selectAllDuplicates: () => {
      update(state => {
        const newSelected = new Set<string>();
        for (const group of state.groups) {
          // Sort and select all except best original
          const sorted = sortGroupFiles(group.files);
          for (let i = 1; i < sorted.length; i++) {
            newSelected.add(`${sorted[i].id}:${sorted[i].path}`);
          }
        }
        return { ...state, selectedFiles: newSelected };
      });
    },

    selectGroupDuplicates: (groupId: number) => {
      update(state => {
        const group = state.groups.find(g => g.id === groupId);
        if (!group) return state;

        const newSelected = new Set(state.selectedFiles);
        const sorted = sortGroupFiles(group.files);
        for (let i = 1; i < sorted.length; i++) {
          newSelected.add(`${sorted[i].id}:${sorted[i].path}`);
        }
        return { ...state, selectedFiles: newSelected };
      });
    },

    clearSelection: () => {
      update(state => ({ ...state, selectedFiles: new Set() }));
    },

    getSelectedCount: () => {
      return getState().selectedFiles.size;
    },

    getSelectedFiles: (): { id: number; path: string }[] => {
      const state = getState();
      return Array.from(state.selectedFiles).map(key => {
        const colonIdx = key.indexOf(':');
        return {
          id: parseInt(key.substring(0, colonIdx)),
          path: key.substring(colonIdx + 1)
        };
      });
    },

    // Delete operations
    deleteFile: async (fileId: number, filePath: string) => {
      try {
        await invoke('delete_duplicate', { fileId, filePath });

        update(state => {
          const newGroups = state.groups
            .map(group => {
              const hasFile = group.files.some(f => f.id === fileId);
              if (!hasFile) return group;

              return {
                ...group,
                files: group.files.filter(f => f.id !== fileId),
              };
            })
            .filter(group => group.files.length > 1);

          const newSelected = new Set(state.selectedFiles);
          newSelected.delete(`${fileId}:${filePath}`);

          return {
            ...state,
            groups: newGroups,
            totalGroups: newGroups.length,
            totalWastedSpace: newGroups.reduce((sum, g) => sum + g.wasted_space, 0),
            selectedFiles: newSelected,
          };
        });
      } catch (e) {
        console.error('Failed to delete file:', e);
        throw e;
      }
    },

    deleteSelected: async (): Promise<{ deleted: number; errors: string[] }> => {
      const currentState = getState();

      if (currentState.selectedFiles.size === 0) {
        return { deleted: 0, errors: [] };
      }

      const filesToDelete: [number, string][] = Array.from(currentState.selectedFiles).map(key => {
        const colonIdx = key.indexOf(':');
        return [
          parseInt(key.substring(0, colonIdx)),
          key.substring(colonIdx + 1)
        ];
      });

      if (filesToDelete.length === 0) {
        return { deleted: 0, errors: ['No valid files to delete'] };
      }

      try {
        const result = await invoke<BatchDeleteResponse>('delete_duplicates_batch', {
          files: filesToDelete,
        });

        if (result.errors && result.errors.length > 0) {
          console.warn('Some files failed to delete:', result.errors);
        }

        // Refresh duplicates after batch delete
        const response = await invoke<DuplicateResponse>('find_duplicates', {
          minSize: null,
        });

        update(state => ({
          ...state,
          groups: response.groups,
          totalGroups: response.total_groups,
          totalWastedSpace: response.total_wasted_space,
          detectionTimeMs: response.detection_time_ms,
          selectedFiles: new Set(),
        }));

        return result;
      } catch (e) {
        console.error('Batch delete failed:', e);
        throw e;
      }
    },

    clear: () => {
      set({
        groups: [],
        similarGroups: [],
        totalGroups: 0,
        totalWastedSpace: 0,
        detectionTimeMs: 0,
        totalSimilarGroups: 0,
        totalSimilarImages: 0,
        similarDetectionTimeMs: 0,
        activeTab: 'exact',
        loading: false,
        similarLoading: false,
        error: null,
        progress: null,
        selectedFiles: new Set(),
        sortBy: 'wastedSpace',
        sortOrder: 'desc',
        filterExtensions: [],
        filterMinSize: 0,
        searchQuery: ''
      });
    },
  };
}

export const duplicatesStore = createDuplicatesStore();
