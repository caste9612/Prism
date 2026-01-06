import { writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import type {
  SizeCategory,
  ExtensionCategory,
  FolderSize,
  FileTypeCategory,
  FolderItem,
  SimilarImageGroup,
  SimilarImagesResponse,
} from '$lib/types';

interface AnalyticsState {
  sizeDistribution: SizeCategory[];
  extensionDistribution: ExtensionCategory[];
  folderSizes: FolderSize[];
  similarImages: SimilarImageGroup[];
  fileTypeCategories: FileTypeCategory[];
  folderContents: FolderItem[];
  currentPath: string | null;
  loading: boolean;
  pendingRequests: number; // Track concurrent requests
  error: string | null;
}

function createAnalyticsStore() {
  const { subscribe, set, update } = writable<AnalyticsState>({
    sizeDistribution: [],
    extensionDistribution: [],
    folderSizes: [],
    similarImages: [],
    fileTypeCategories: [],
    folderContents: [],
    currentPath: null,
    loading: false,
    pendingRequests: 0,
    error: null,
  });

  // Helper to increment pending requests
  const startRequest = () => {
    update(state => ({
      ...state,
      pendingRequests: state.pendingRequests + 1,
      loading: true,
      error: null,
    }));
  };

  // Helper to decrement pending requests
  const endRequest = () => {
    update(state => {
      const newPending = Math.max(0, state.pendingRequests - 1);
      return {
        ...state,
        pendingRequests: newPending,
        loading: newPending > 0, // Only stop loading when all requests complete
      };
    });
  };

  // Helper to handle errors with proper message extraction
  const handleError = (e: unknown): string => {
    return e instanceof Error ? e.message : String(e);
  };

  return {
    subscribe,

    loadSizeDistribution: async () => {
      startRequest();
      try {
        const data = await invoke<SizeCategory[]>('get_size_distribution');
        update(state => ({ ...state, sizeDistribution: data }));
        return data;
      } catch (e) {
        update(state => ({ ...state, error: handleError(e) }));
        throw e;
      } finally {
        endRequest();
      }
    },

    loadExtensionDistribution: async (limit?: number) => {
      startRequest();
      try {
        const data = await invoke<ExtensionCategory[]>('get_extension_distribution', {
          limit: limit ?? 20,
        });
        update(state => ({ ...state, extensionDistribution: data }));
        return data;
      } catch (e) {
        update(state => ({ ...state, error: handleError(e) }));
        throw e;
      } finally {
        endRequest();
      }
    },

    loadFolderSizes: async (rootPath?: string, depth?: number) => {
      startRequest();
      try {
        const data = await invoke<FolderSize[]>('get_folder_sizes', {
          rootPath: rootPath ?? null,
          depth: depth ?? 2,
        });
        update(state => ({ ...state, folderSizes: data }));
        return data;
      } catch (e) {
        update(state => ({ ...state, error: handleError(e) }));
        throw e;
      } finally {
        endRequest();
      }
    },

    loadFileTypeDistribution: async () => {
      startRequest();
      try {
        const data = await invoke<FileTypeCategory[]>('get_file_type_distribution');
        update(state => ({ ...state, fileTypeCategories: data }));
        return data;
      } catch (e) {
        update(state => ({ ...state, error: handleError(e) }));
        throw e;
      } finally {
        endRequest();
      }
    },

    loadFolderContents: async (path?: string, limit?: number) => {
      startRequest();
      try {
        const data = await invoke<FolderItem[]>('get_folder_contents', {
          path: path ?? null,
          limit: limit ?? 20,
        });
        update(state => ({ ...state, folderContents: data, currentPath: path ?? null }));
        return data;
      } catch (e) {
        update(state => ({ ...state, error: handleError(e) }));
        throw e;
      } finally {
        endRequest();
      }
    },

    findSimilarImages: async (threshold?: number) => {
      startRequest();
      try {
        const response = await invoke<SimilarImagesResponse>('find_similar_images', {
          threshold: threshold ?? 10,
        });
        update(state => ({
          ...state,
          similarImages: response.groups,
        }));
        return response;
      } catch (e) {
        update(state => ({ ...state, error: handleError(e) }));
        throw e;
      } finally {
        endRequest();
      }
    },

    loadAll: async () => {
      startRequest();
      try {
        // Use Promise.allSettled for resilience - partial failures don't break everything
        const results = await Promise.allSettled([
          invoke<SizeCategory[]>('get_size_distribution'),
          invoke<ExtensionCategory[]>('get_extension_distribution', { limit: 20 }),
          invoke<FolderSize[]>('get_folder_sizes', { depth: 3 }),
        ]);

        // Extract successful results, use empty arrays for failures
        const sizeData = results[0].status === 'fulfilled' ? results[0].value : [];
        const extData = results[1].status === 'fulfilled' ? results[1].value : [];
        const folderData = results[2].status === 'fulfilled' ? results[2].value : [];

        // Collect any errors
        const errors = results
          .filter((r): r is PromiseRejectedResult => r.status === 'rejected')
          .map(r => handleError(r.reason));

        update(state => ({
          ...state,
          sizeDistribution: sizeData,
          extensionDistribution: extData,
          folderSizes: folderData,
          error: errors.length > 0 ? `Partial load failure: ${errors.join('; ')}` : null,
        }));

        // Return info about what loaded
        return {
          sizeDistribution: sizeData,
          extensionDistribution: extData,
          folderSizes: folderData,
          errors,
        };
      } catch (e) {
        update(state => ({ ...state, error: handleError(e) }));
        throw e;
      } finally {
        endRequest();
      }
    },

    clear: () => {
      set({
        sizeDistribution: [],
        extensionDistribution: [],
        folderSizes: [],
        similarImages: [],
        fileTypeCategories: [],
        folderContents: [],
        currentPath: null,
        loading: false,
        pendingRequests: 0,
        error: null,
      });
    },
  };
}

export const analyticsStore = createAnalyticsStore();
