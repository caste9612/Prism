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

  return {
    subscribe,

    /**
     * Show a notification
     */
    notify: (message: string, type: 'success' | 'error' | 'info' = 'info') => {
      let id: number;

      update(state => {
        id = state.nextId;
        return {
          notifications: [...state.notifications, { id, message, type }],
          nextId: state.nextId + 1,
        };
      });

      // Auto-dismiss after duration
      setTimeout(() => {
        update(state => ({
          ...state,
          notifications: state.notifications.filter(n => n.id !== id),
        }));
      }, NOTIFICATION_DURATION);

      return id!;
    },

    /**
     * Dismiss a specific notification
     */
    dismiss: (id: number) => {
      update(state => ({
        ...state,
        notifications: state.notifications.filter(n => n.id !== id),
      }));
    },

    /**
     * Clear all notifications
     */
    clear: () => {
      update(state => ({
        ...state,
        notifications: [],
      }));
    },

    /**
     * Show success notification
     */
    success: (message: string) => {
      return createNotificationStore().notify(message, 'success');
    },

    /**
     * Show error notification
     */
    error: (message: string) => {
      return createNotificationStore().notify(message, 'error');
    },

    /**
     * Show info notification
     */
    info: (message: string) => {
      return createNotificationStore().notify(message, 'info');
    },
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
