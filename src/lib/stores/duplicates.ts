import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import type { DuplicateFile, DuplicateGroup, DuplicateResponse } from '$lib/types';

interface DuplicatesState {
  groups: DuplicateGroup[];
  totalGroups: number;
  totalWastedSpace: number;
  detectionTimeMs: number;
  loading: boolean;
  error: string | null;
  selectedFiles: Set<number>;
}

interface BatchDeleteResponse {
  deleted: number;
  errors: string[];
}

function createDuplicatesStore() {
  const store = writable<DuplicatesState>({
    groups: [],
    totalGroups: 0,
    totalWastedSpace: 0,
    detectionTimeMs: 0,
    loading: false,
    error: null,
    selectedFiles: new Set(),
  });

  const { subscribe, set, update } = store;

  // Helper to get current state safely
  const getState = () => get(store);

  return {
    subscribe,

    findDuplicates: async (minSize?: number) => {
      update(state => ({ ...state, loading: true, error: null }));

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
          selectedFiles: new Set(),
        }));

        return response;
      } catch (e) {
        console.error('Duplicate detection failed:', e);
        update(state => ({
          ...state,
          loading: false,
          error: String(e),
        }));
        throw e;
      }
    },

    toggleFileSelection: (fileId: number) => {
      update(state => {
        const newSelected = new Set(state.selectedFiles);
        if (newSelected.has(fileId)) {
          newSelected.delete(fileId);
        } else {
          newSelected.add(fileId);
        }
        return { ...state, selectedFiles: newSelected };
      });
    },

    selectAllDuplicates: () => {
      update(state => {
        const newSelected = new Set<number>();
        for (const group of state.groups) {
          // Select all files except the original in each group
          for (const file of group.files) {
            if (!file.is_original) {
              newSelected.add(file.id);
            }
          }
        }
        return { ...state, selectedFiles: newSelected };
      });
    },

    clearSelection: () => {
      update(state => ({ ...state, selectedFiles: new Set() }));
    },

    deleteFile: async (fileId: number, filePath: string) => {
      try {
        await invoke('delete_duplicate', { fileId, filePath });

        // Remove from state - only update the affected group
        update(state => {
          const newGroups = state.groups
            .map(group => {
              // Only create new object if this group contains the file
              const hasFile = group.files.some(f => f.id === fileId);
              if (!hasFile) return group;

              return {
                ...group,
                files: group.files.filter(f => f.id !== fileId),
              };
            })
            .filter(group => group.files.length > 1); // Remove groups with less than 2 files

          const newSelected = new Set(state.selectedFiles);
          newSelected.delete(fileId);

          return {
            ...state,
            groups: newGroups,
            totalGroups: newGroups.length,
            selectedFiles: newSelected,
          };
        });
      } catch (e) {
        console.error('Failed to delete file:', e);
        throw e;
      }
    },

    deleteSelected: async (): Promise<{ deleted: number; errors: string[] }> => {
      // Use getState() for safe state access
      const currentState = getState();

      if (currentState.selectedFiles.size === 0) {
        return { deleted: 0, errors: [] };
      }

      // Build list of files to delete with validation
      const filesToDelete: [number, string][] = [];
      for (const group of currentState.groups) {
        for (const file of group.files) {
          if (currentState.selectedFiles.has(file.id)) {
            // Validate file data before adding
            if (typeof file.id === 'number' && typeof file.path === 'string' && file.path.length > 0) {
              filesToDelete.push([file.id, file.path]);
            }
          }
        }
      }

      if (filesToDelete.length === 0) {
        return { deleted: 0, errors: ['No valid files to delete'] };
      }

      try {
        const result = await invoke<BatchDeleteResponse>('delete_duplicates_batch', {
          files: filesToDelete,
        });

        // Check for partial failures
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
        totalGroups: 0,
        totalWastedSpace: 0,
        detectionTimeMs: 0,
        loading: false,
        error: null,
        selectedFiles: new Set(),
      });
    },
  };
}

export const duplicatesStore = createDuplicatesStore();
