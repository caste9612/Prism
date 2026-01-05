import { writable } from 'svelte/store';
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

function createDuplicatesStore() {
  const { subscribe, set, update } = writable<DuplicatesState>({
    groups: [],
    totalGroups: 0,
    totalWastedSpace: 0,
    detectionTimeMs: 0,
    loading: false,
    error: null,
    selectedFiles: new Set(),
  });

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

        // Remove from state
        update(state => {
          const newGroups = state.groups.map(group => ({
            ...group,
            files: group.files.filter(f => f.id !== fileId),
          })).filter(group => group.files.length > 1); // Remove groups with less than 2 files

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

    deleteSelected: async () => {
      let state: DuplicatesState;
      const unsubscribe = subscribe(s => state = s);
      unsubscribe();

      if (state!.selectedFiles.size === 0) return 0;

      // Build list of files to delete
      const filesToDelete: [number, string][] = [];
      for (const group of state!.groups) {
        for (const file of group.files) {
          if (state!.selectedFiles.has(file.id)) {
            filesToDelete.push([file.id, file.path]);
          }
        }
      }

      try {
        const deleted = await invoke<number>('delete_duplicates_batch', {
          files: filesToDelete,
        });

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

        return deleted;
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
