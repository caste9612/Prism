/**
 * Global application store for shared state like notifications
 */
import { writable, derived } from 'svelte/store';
import type { Notification } from '$lib/types';

// ============================================================================
// Notification Store
// ============================================================================

interface NotificationState {
  notifications: Notification[];
  nextId: number;
}

function createNotificationStore() {
  const { subscribe, update } = writable<NotificationState>({
    notifications: [],
    nextId: 1,
  });

  const NOTIFICATION_DURATION = 4000;

  // Track active timeouts for cleanup
  const timeoutMap = new Map<number, ReturnType<typeof setTimeout>>();

  // Define notify function separately so helper methods can reference it
  const notify = (message: string, type: 'success' | 'error' | 'info' = 'info') => {
    let id: number;

    update(state => {
      id = state.nextId;
      return {
        notifications: [...state.notifications, { id, message, type }],
        nextId: state.nextId + 1,
      };
    });

    // Auto-dismiss after duration with tracked timeout
    const timeoutId = setTimeout(() => {
      timeoutMap.delete(id);
      update(state => ({
        ...state,
        notifications: state.notifications.filter(n => n.id !== id),
      }));
    }, NOTIFICATION_DURATION);

    timeoutMap.set(id!, timeoutId);
    return id!;
  };

  const dismiss = (id: number) => {
    // Clear the associated timeout to prevent memory leak
    const timeoutId = timeoutMap.get(id);
    if (timeoutId) {
      clearTimeout(timeoutId);
      timeoutMap.delete(id);
    }
    update(state => ({
      ...state,
      notifications: state.notifications.filter(n => n.id !== id),
    }));
  };

  const clear = () => {
    // Clear all pending timeouts
    for (const timeoutId of timeoutMap.values()) {
      clearTimeout(timeoutId);
    }
    timeoutMap.clear();
    update(state => ({
      ...state,
      notifications: [],
    }));
  };

  return {
    subscribe,
    notify,
    dismiss,
    clear,
    // Helper methods now correctly use the same notify function
    success: (message: string) => notify(message, 'success'),
    error: (message: string) => notify(message, 'error'),
    info: (message: string) => notify(message, 'info'),
  };
}

export const notificationStore = createNotificationStore();

// Derived store for just the notifications array
export const notifications = derived(
  notificationStore,
  $store => $store.notifications
);

// ============================================================================
// Global Loading State
// ============================================================================

interface LoadingState {
  isScanning: boolean;
  scanProgress: number;
  currentOperation: string | null;
}

function createLoadingStore() {
  const { subscribe, set, update } = writable<LoadingState>({
    isScanning: false,
    scanProgress: 0,
    currentOperation: null,
  });

  return {
    subscribe,

    startScanning: () => {
      update(state => ({ ...state, isScanning: true, scanProgress: 0 }));
    },

    updateProgress: (progress: number, operation?: string) => {
      update(state => ({
        ...state,
        scanProgress: progress,
        currentOperation: operation ?? state.currentOperation,
      }));
    },

    stopScanning: () => {
      update(state => ({ ...state, isScanning: false, scanProgress: 100 }));
    },

    setOperation: (operation: string | null) => {
      update(state => ({ ...state, currentOperation: operation }));
    },

    reset: () => {
      set({ isScanning: false, scanProgress: 0, currentOperation: null });
    },
  };
}

export const loadingStore = createLoadingStore();
