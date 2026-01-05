import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { get } from 'svelte/store';
import { notificationStore, notifications, loadingStore } from './app';

describe('notificationStore', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    notificationStore.clear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('should start with empty notifications', () => {
    const state = get(notificationStore);
    expect(state.notifications).toHaveLength(0);
  });

  it('should add a notification', () => {
    notificationStore.notify('Test message', 'info');
    const state = get(notificationStore);
    expect(state.notifications).toHaveLength(1);
    expect(state.notifications[0].message).toBe('Test message');
    expect(state.notifications[0].type).toBe('info');
  });

  it('should add notifications with different types', () => {
    notificationStore.notify('Info', 'info');
    notificationStore.notify('Success', 'success');
    notificationStore.notify('Error', 'error');

    const state = get(notificationStore);
    expect(state.notifications).toHaveLength(3);
    expect(state.notifications[0].type).toBe('info');
    expect(state.notifications[1].type).toBe('success');
    expect(state.notifications[2].type).toBe('error');
  });

  it('should auto-dismiss notifications after 4 seconds', () => {
    notificationStore.notify('Test message', 'info');
    expect(get(notificationStore).notifications).toHaveLength(1);

    // Fast-forward 4 seconds
    vi.advanceTimersByTime(4000);

    expect(get(notificationStore).notifications).toHaveLength(0);
  });

  it('should dismiss specific notification', () => {
    const id = notificationStore.notify('Test 1', 'info');
    notificationStore.notify('Test 2', 'info');

    expect(get(notificationStore).notifications).toHaveLength(2);

    notificationStore.dismiss(id);

    const remaining = get(notificationStore).notifications;
    expect(remaining).toHaveLength(1);
    expect(remaining[0].message).toBe('Test 2');
  });

  it('should clear all notifications', () => {
    notificationStore.notify('Test 1', 'info');
    notificationStore.notify('Test 2', 'info');
    notificationStore.notify('Test 3', 'info');

    expect(get(notificationStore).notifications).toHaveLength(3);

    notificationStore.clear();

    expect(get(notificationStore).notifications).toHaveLength(0);
  });

  it('should increment notification IDs', () => {
    const id1 = notificationStore.notify('First', 'info');
    const id2 = notificationStore.notify('Second', 'info');
    const id3 = notificationStore.notify('Third', 'info');

    expect(id1).toBeLessThan(id2);
    expect(id2).toBeLessThan(id3);
  });
});

describe('notifications derived store', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    notificationStore.clear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('should derive notifications array from store', () => {
    notificationStore.notify('Test', 'info');
    const notifs = get(notifications);
    expect(notifs).toHaveLength(1);
    expect(notifs[0].message).toBe('Test');
  });
});

describe('loadingStore', () => {
  it('should start with initial state', () => {
    loadingStore.reset();
    const state = get(loadingStore);
    expect(state.isScanning).toBe(false);
    expect(state.scanProgress).toBe(0);
    expect(state.currentOperation).toBeNull();
  });

  it('should start scanning', () => {
    loadingStore.startScanning();
    const state = get(loadingStore);
    expect(state.isScanning).toBe(true);
    expect(state.scanProgress).toBe(0);
  });

  it('should update progress', () => {
    loadingStore.startScanning();
    loadingStore.updateProgress(50, 'Processing files...');

    const state = get(loadingStore);
    expect(state.scanProgress).toBe(50);
    expect(state.currentOperation).toBe('Processing files...');
  });

  it('should stop scanning', () => {
    loadingStore.startScanning();
    loadingStore.updateProgress(75);
    loadingStore.stopScanning();

    const state = get(loadingStore);
    expect(state.isScanning).toBe(false);
    expect(state.scanProgress).toBe(100);
  });

  it('should set operation', () => {
    loadingStore.setOperation('Indexing files');
    expect(get(loadingStore).currentOperation).toBe('Indexing files');

    loadingStore.setOperation(null);
    expect(get(loadingStore).currentOperation).toBeNull();
  });

  it('should reset state', () => {
    loadingStore.startScanning();
    loadingStore.updateProgress(75, 'Testing');
    loadingStore.reset();

    const state = get(loadingStore);
    expect(state.isScanning).toBe(false);
    expect(state.scanProgress).toBe(0);
    expect(state.currentOperation).toBeNull();
  });
});
