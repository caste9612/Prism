import { describe, expect, it } from 'vitest';
import { childFolder, copyJob, coverage, crumbs, parentFolder, percent } from './backup';
import { mockBackup } from './mock-backup';
import { sampleVolumes } from './mock';

describe('backup helpers', () => {
  // F13-AC3: the shares checked by content and by metadata never round up to 100%.
  it('coverage of a check', () => {
    const c = coverage({
      reportId: 1,
      total: { files: 1000, bytes: 0 },
      byContent: { files: 999, bytes: 0 },
      byMetadata: { files: 0, bytes: 0 },
      missing: { files: 1, bytes: 10 },
    });
    expect(c).toEqual({ content: 99, metadata: 0, missing: 1, complete: false });
    expect(percent({ files: 3, bytes: 0 }, { files: 0, bytes: 0 })).toBe(0);
  });

  // F13-AC4, F14-AC3: navigation of the result tree.
  it('folder paths', () => {
    expect(crumbs('Foto\\2024\\Vacanza')).toEqual([
      { key: 'Foto', name: 'Foto' },
      { key: 'Foto\\2024', name: '2024' },
      { key: 'Foto\\2024\\Vacanza', name: 'Vacanza' },
    ]);
    expect(crumbs('')).toEqual([]);
    expect(parentFolder('Foto\\2024')).toBe('Foto');
    expect(parentFolder('Foto')).toBe('');
    expect(childFolder('', 'Foto')).toBe('Foto');
    expect(childFolder('Foto', '2024')).toBe('Foto\\2024');
    expect(copyJob([], null)).toBeNull();
  });
});

describe('simulated backups follow the backend rules', () => {
  const backup = mockBackup(() => sampleVolumes());
  const data = { volumeId: 2, folder: '' };

  it('overlapping folders and missing destinations are refused', () => {
    expect(() => backup.verify(data, [])).toThrow();
    expect(() => backup.verify(data, [{ volumeId: 2, folder: 'Foto' }])).toThrow();
    expect(() => backup.compare(data, { volumeId: 2, folder: '' }, true)).toThrow();
  });

  it('levels of the tree carry the totals of their subfolders', () => {
    const r = backup.verify(data, [{ volumeId: 3, folder: '' }]);
    const top = backup.folder(r.reportId, '', { text: null, minSize: 0, statuses: [] });
    expect(top.folders.map((f) => f.key)).toEqual(['Foto', 'Documenti']);
    expect(top.tallies[0]!.files).toBe(5);
    const foto = backup.folder(r.reportId, 'Foto', { text: null, minSize: 2_000_000, statuses: [] });
    expect(foto.tallies[0]!.files).toBe(3);
  });

  // F15-AC1: both disks must be connected.
  it('copy preview needs both disks connected', () => {
    const r = backup.verify(data, [{ volumeId: 3, folder: '' }]);
    expect(() => backup.preview(r.reportId, { volumeId: 3, folder: '' })).toThrow();
    const p = backup.preview(r.reportId, { volumeId: 1, folder: '\\Copia\\' });
    expect(p.destPath).toBe('C:\\Copia');
    expect(p.enoughSpace).toBe(true);
  });
});
