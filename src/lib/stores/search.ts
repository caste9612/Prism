import { writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import type { SearchResult, SearchResponse } from '$lib/types';

interface SearchState {
  query: string;
  results: SearchResult[];
  totalCount: number;
  queryTimeMs: number;
  loading: boolean;
  error: string | null;
}

function createSearchStore() {
  const { subscribe, set, update } = writable<SearchState>({
    query: '',
    results: [],
    totalCount: 0,
    queryTimeMs: 0,
    loading: false,
    error: null,
  });

  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  const DEBOUNCE_MS = 150;

  return {
    subscribe,

    setQuery: (query: string) => {
      update(state => ({ ...state, query }));

      // Clear previous timer
      if (debounceTimer) {
        clearTimeout(debounceTimer);
      }

      // Don't search for very short queries
      if (query.trim().length < 2) {
        update(state => ({
          ...state,
          results: [],
          totalCount: 0,
          queryTimeMs: 0,
          loading: false,
          error: null,
        }));
        return;
      }

      // Debounce the search
      update(state => ({ ...state, loading: true }));

      debounceTimer = setTimeout(async () => {
        try {
          const response = await invoke<SearchResponse>('search_files_advanced', {
            request: {
              query: query.trim(),
              limit: 100,
              offset: 0,
            },
          });

          update(state => ({
            ...state,
            results: response.results,
            totalCount: response.total_count,
            queryTimeMs: response.query_time_ms,
            loading: false,
            error: null,
          }));
        } catch (e) {
          console.error('Search failed:', e);
          update(state => ({
            ...state,
            results: [],
            totalCount: 0,
            loading: false,
            error: String(e),
          }));
        }
      }, DEBOUNCE_MS);
    },

    search: async (query: string, options?: { limit?: number; offset?: number; sortBy?: string; sortOrder?: string }) => {
      update(state => ({ ...state, query, loading: true, error: null }));

      try {
        const response = await invoke<SearchResponse>('search_files_advanced', {
          request: {
            query: query.trim(),
            limit: options?.limit ?? 100,
            offset: options?.offset ?? 0,
            sort_by: options?.sortBy,
            sort_order: options?.sortOrder,
          },
        });

        update(state => ({
          ...state,
          results: response.results,
          totalCount: response.total_count,
          queryTimeMs: response.query_time_ms,
          loading: false,
        }));

        return response;
      } catch (e) {
        console.error('Search failed:', e);
        update(state => ({
          ...state,
          results: [],
          loading: false,
          error: String(e),
        }));
        throw e;
      }
    },

    clear: () => {
      if (debounceTimer) {
        clearTimeout(debounceTimer);
      }
      set({
        query: '',
        results: [],
        totalCount: 0,
        queryTimeMs: 0,
        loading: false,
        error: null,
      });
    },
  };
}

export const searchStore = createSearchStore();
