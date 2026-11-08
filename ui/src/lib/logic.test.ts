import { describe, expect, it } from 'vitest';
import { dictionaries, interpolate, keys, setLanguage, t } from './i18n.svelte';
import { ago, formatBytes, formatCount, usedFraction } from './format';
import { cardJob, displayName, groupPrompts, newVolumePrompts, primaryAction, visibleVolumes } from './catalog';
import { mockBackend, sampleVolumes } from './mock';

describe('i18n', () => {
  it('English and Italian have exactly the same keys, none empty', () => {
    const en = keys(dictionaries.en).sort();
    const it = keys(dictionaries.it).sort();
    expect(it).toEqual(en);
    for (const k of en) {
      expect(t(k).length).toBeGreaterThan(0);
    }
  });

  it('placeholders are the same in both languages', () => {
    for (const key of keys(dictionaries.en)) {
      setLanguage('en');
      const en = t(key).match(/\{\w+\}/g)?.sort() ?? [];
      setLanguage('it');
      const ita = t(key).match(/\{\w+\}/g)?.sort() ?? [];
      expect(ita, key).toEqual(en);
    }
  });

  it('translates with parameters and falls back to the key', () => {
    setLanguage('it');
    expect(t('catalog.files', { count: 3 })).toBe('3 file');
    setLanguage('en');
    expect(t('catalog.files', { count: 3 })).toBe('3 files');
    expect(t('no.such.key')).toBe('no.such.key');
    expect(interpolate('{a} and {b}', { a: 1 })).toBe('1 and {b}');
  });
});

describe('format', () => {
  it('sizes in decimal units', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(1536)).toBe('1.5 KB');
    expect(formatBytes(439_000_000_000)).toBe('439 GB');
    expect(formatBytes(1_500_000_000, 'it')).toBe('1,5 GB');
    expect(formatBytes(-1)).toBe('—');
  });

  it('counts and relative times', () => {
    expect(formatCount(1234567, 'en')).toBe('1,234,567');
    expect(formatCount(1234567, 'it')).toBe('1.234.567');
    const now = 1_000_000_000;
    expect(ago(now - 10_000, now)).toEqual({ key: 'time.justNow' });
    expect(ago(now - 5 * 60_000, now)).toEqual({ key: 'time.minutes', count: 5 });
    expect(ago(now - 3 * 3_600_000, now)).toEqual({ key: 'time.hours', count: 3 });
    expect(ago(now - 50 * 3_600_000, now)).toEqual({ key: 'time.days', count: 2 });
  });

  it('used fraction', () => {
    expect(usedFraction(100, 25)).toBe(0.75);
    expect(usedFraction(null, 25)).toBeNull();
    expect(usedFraction(0, 0)).toBeNull();
  });
});

describe('catalog screen', () => {
  // F01-AC1: offline volumes stay visible; ignored ones are hidden (F02-AC4).
  it('groups volumes, connected first, ignored hidden', () => {
    const vols = sampleVolumes();
    vols[1]!.status = 'ignored';
    const g = visibleVolumes(vols);
    expect(g.online.map((v) => v.name)).toEqual(['KINGSTON', 'NAS Big-Storage', 'Windows']);
    expect(g.offline.map((v) => v.name)).toEqual(['Backup Foto']);
  });

  // F02-AC1: a connected disk never indexed is proposed until dismissed.
  it('new-disk prompts', () => {
    const vols = sampleVolumes();
    expect(newVolumePrompts(vols, new Set()).map((v) => v.id)).toEqual([5]);
    expect(newVolumePrompts(vols, new Set([5]))).toEqual([]);
  });

  // First launch: every connected disk is new; the prompts collapse into one panel.
  it('many new disks are grouped', () => {
    const vols = sampleVolumes().map((v) => ({ ...v, status: 'new' as const, job: null }));
    const prompts = newVolumePrompts(vols, new Set());
    expect(prompts.length).toBe(vols.filter((v) => v.online).length);
    expect(groupPrompts(prompts)).toBe(true);
    expect(groupPrompts(prompts.slice(0, 2))).toBe(false);
  });

  it('display name of unlabelled disks', () => {
    const [c] = sampleVolumes();
    expect(displayName(c!, 'Local disk')).toBe(c!.name);
    const bare = { ...c!, userName: null, osLabel: '', name: 'C:\\', mount: 'C:\\' };
    expect(displayName(bare, 'Internal disk')).toBe('Internal disk (C:)');
    const unc = { ...bare, name: '\\\\nas\\share', mount: '\\\\nas\\share' };
    // A share reached by its path shows the share name (T11: shares found without a letter).
    expect(displayName(unc, 'Network drive')).toBe('Network drive (share)');
  });

  // F01-AC3: indexing and updates show on the card; automatic background updates do not.
  it('job shown on a card', () => {
    const job = (id: number, kind: string, state: string) =>
      ({ id, kind, volumeId: 1, state, done: 0, total: 0, phase: '', error: null });
    expect(cardJob([job(1, 'journal', 'running'), job(2, 'refresh', 'queued')], 1)).toBeNull();
    expect(cardJob([job(1, 'journal', 'running'), job(3, 'update', 'running')], 1)?.id).toBe(3);
    expect(cardJob([job(4, 'index', 'done')], 1)).toBeNull();
    expect(cardJob([job(5, 'index', 'queued')], 2)).toBeNull();
  });

  it('primary action of a card', () => {
    const [c, , backup, , stick] = sampleVolumes();
    expect(primaryAction(c!)).toBe('update');
    expect(primaryAction(backup!)).toBeNull(); // offline
    expect(primaryAction(stick!)).toBe('index');
  });
});

describe('simulated backend', () => {
  it('indexing a disk emits job progress and ends indexed', async () => {
    const b = mockBackend();
    const seen: string[] = [];
    await b.onJobsChanged((jobs) => seen.push(jobs[0]!.state));
    await b.indexVolume(5);
    await new Promise((r) => setTimeout(r, 2200));
    const stick = (await b.volumes()).find((v) => v.id === 5)!;
    expect(stick.status).toBe('indexed');
    expect(stick.job).toBeNull();
    expect(seen.at(-1)).toBe('done');
  });

  it('offline disks cannot be indexed; labels and ignore round-trip', async () => {
    const b = mockBackend();
    await expect(b.indexVolume(3)).rejects.toMatchObject({ code: 'volume_offline' });
    await b.setLabels(1, { name: 'Sistema', location: 'PC', notes: null, color: '#10b981' });
    await b.setIgnored(2, true);
    const vols = await b.volumes();
    expect(vols.find((v) => v.id === 1)).toMatchObject({ name: 'Sistema', location: 'PC' });
    expect(vols.find((v) => v.id === 2)?.status).toBe('ignored');
  });
});
