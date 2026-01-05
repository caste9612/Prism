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
    error: null,
  });

  return {
    subscribe,

    loadSizeDistribution: async () => {
      update(state => ({ ...state, loading: true }));
      try {
        const data = await invoke<SizeCategory[]>('get_size_distribution');
        update(state => ({ ...state, sizeDistribution: data, loading: false }));
        return data;
      } catch (e) {
        update(state => ({ ...state, loading: false, error: String(e) }));
        throw e;
      }
    },

    loadExtensionDistribution: async (limit?: number) => {
      update(state => ({ ...state, loading: true }));
      try {
        const data = await invoke<ExtensionCategory[]>('get_extension_distribution', {
          limit: limit ?? 20,
        });
        update(state => ({ ...state, extensionDistribution: data, loading: false }));
        return data;
      } catch (e) {
        update(state => ({ ...state, loading: false, error: String(e) }));
        throw e;
      }
    },

    loadFolderSizes: async (rootPath?: string, depth?: number) => {
      update(state => ({ ...state, loading: true }));
      try {
        const data = await invoke<FolderSize[]>('get_folder_sizes', {
          rootPath: rootPath ?? null,
          depth: depth ?? 2,
        });
        update(state => ({ ...state, folderSizes: data, loading: false }));
        return data;
      } catch (e) {
        update(state => ({ ...state, loading: false, error: String(e) }));
        throw e;
      }
    },

    loadFileTypeDistribution: async () => {
      update(state => ({ ...state, loading: true }));
      try {
        const data = await invoke<FileTypeCategory[]>('get_file_type_distribution');
        update(state => ({ ...state, fileTypeCategories: data, loading: false }));
        return data;
      } catch (e) {
        update(state => ({ ...state, loading: false, error: String(e) }));
        throw e;
      }
    },

    loadFolderContents: async (path?: string, limit?: number) => {
      update(state => ({ ...state, loading: true, currentPath: path ?? null }));
      try {
        const data = await invoke<FolderItem[]>('get_folder_contents', {
          path: path ?? null,
          limit: limit ?? 20,
        });
        update(state => ({ ...state, folderContents: data, loading: false }));
        return data;
      } catch (e) {
        update(state => ({ ...state, loading: false, error: String(e) }));
        throw e;
      }
    },

    findSimilarImages: async (threshold?: number) => {
      update(state => ({ ...state, loading: true }));
      try {
        const response = await invoke<SimilarImagesResponse>('find_similar_images', {
          threshold: threshold ?? 10,
        });
        update(state => ({
          ...state,
          similarImages: response.groups,
          loading: false,
        }));
        return response;
      } catch (e) {
        update(state => ({ ...state, loading: false, error: String(e) }));
        throw e;
      }
    },

    loadAll: async () => {
      update(state => ({ ...state, loading: true }));
      try {
        const [sizeData, extData, folderData] = await Promise.all([
          invoke<SizeCategory[]>('get_size_distribution'),
          invoke<ExtensionCategory[]>('get_extension_distribution', { limit: 20 }),
          invoke<FolderSize[]>('get_folder_sizes', { depth: 3 }),
        ]);

        update(state => ({
          ...state,
          sizeDistribution: sizeData,
          extensionDistribution: extData,
          folderSizes: folderData,
          loading: false,
        }));
      } catch (e) {
        update(state => ({ ...state, loading: false, error: String(e) }));
        throw e;
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
        error: null,
      });
    },
  };
}

export const analyticsStore = createAnalyticsStore();
