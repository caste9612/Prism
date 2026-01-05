/**
 * Store index - re-exports all stores for cleaner imports
 *
 * Usage:
 *   import { stats, searchStore, settingsStore } from '$lib/stores';
 */

export { stats } from './stats';
export { searchStore } from './search';
export { duplicatesStore } from './duplicates';
export { analyticsStore } from './analytics';
export { settingsStore } from './settings';
export { notificationStore, notifications, loadingStore } from './app';
