import { writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import type { AppStats, ScanInfo } from '$lib/types';

interface StatsState {
  totalFiles: number;
  totalSize: number;
  duplicateCount: number;
  lastScan: ScanInfo | null;
}

function createStatsStore() {
  const { subscribe, set, update } = writable<StatsState>({
    totalFiles: 0,
    totalSize: 0,
    duplicateCount: 0,
    lastScan: null,
  });

  return {
    subscribe,
    load: async () => {
      try {
        const result = await invoke<{
          total_files: number;
          total_size: number;
          duplicate_count: number;
          last_scan: any | null;
        }>('get_stats');

        set({
          totalFiles: result.total_files,
          totalSize: result.total_size,
          duplicateCount: result.duplicate_count,
          lastScan: result.last_scan,
        });
      } catch (e) {
        console.error('Failed to load stats:', e);
      }
    },
    reset: () => {
      set({
        totalFiles: 0,
        totalSize: 0,
        duplicateCount: 0,
        lastScan: null,
      });
    },
  };
}

export const stats = createStatsStore();
