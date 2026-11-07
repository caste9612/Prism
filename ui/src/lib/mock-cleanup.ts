// Simulated cleanup proposals (browser development, tests), with the shapes of the backend.
import type { CleanupGroupView } from './bindings/CleanupGroupView';
import type { CleanupResultView } from './bindings/CleanupResultView';
import type { CleanupView } from './bindings/CleanupView';

const MB = 1_000_000;
const GB = 1_000_000_000;
const DAY = 86_400_000;

function items(base: number, list: [string, number, boolean?][], now: number) {
  return list.map(([path, size, isDir], i) => ({
    id: base + i,
    path,
    isDir: isDir ?? false,
    size,
    files: isDir ? 3 : 1,
    modified: now - (i + 2) * 40 * DAY,
  }));
}

function sample(now = Date.UTC(2026, 10, 7)): CleanupGroupView[] {
  const g = (
    category: string,
    volumeId: number,
    list: ReturnType<typeof items>,
    extra: Partial<CleanupGroupView> = {},
  ): CleanupGroupView => ({
    category,
    volumeId,
    owner: null,
    blockedBy: null,
    permanent: false,
    items: list,
    count: list.length,
    total: list.reduce((a, i) => a + i.size, 0),
    ...extra,
  });
  return [
    g('temp', 1, items(9000, [
      ['Users\\me\\AppData\\Local\\Temp\\setup-4411.log', 12 * MB],
      ['Users\\me\\AppData\\Local\\Temp\\vs-installer\\payload.vsix', 390 * MB],
      ['Users\\me\\AppData\\Local\\Temp\\chrome_drag1234.tmp', 2 * MB],
      ['Windows\\Temp\\cbs-2026.log', 48 * MB],
    ], now)),
    g('empty_folders', 1, items(9100, [
      ['Users\\me\\Projects\\old\\build', 0, true],
      ['Old Stuff\\empty', 0, true],
    ], now)),
    g('large_old', 1, items(9200, [
      ['Users\\me\\Videos\\holiday-2019.mkv', 2.4 * GB],
      ['Old Backups\\image-2021.iso', 4.2 * GB],
    ], now)),
    g('browser_cache', 1, items(9300, [
      ['Users\\me\\AppData\\Local\\Google\\Chrome\\User Data\\Default\\Cache', 418 * MB, true],
      ['Users\\me\\AppData\\Local\\Google\\Chrome\\User Data\\Default\\Code Cache', 395 * MB, true],
    ], now), { owner: 'Chrome' }),
    g('browser_cache', 1, items(9400, [
      ['Users\\me\\AppData\\Local\\Mozilla\\Firefox\\Profiles\\abc.default\\cache2', 401 * MB, true],
    ], now), { owner: 'Firefox', blockedBy: 'Firefox' }),
    g('large_old', 4, items(9500, [['Archivio\\vecchio-backup.zip', 7.8 * GB]], now), { permanent: true }),
  ];
}

let groups: CleanupGroupView[] | null = null;

export function mockCleanupScan(): CleanupView {
  groups ??= sample();
  return {
    groups: groups.filter((g) => g.items.length > 0),
    recycleBins: [{ volumeId: 1, items: 12, bytes: 1.3 * GB }],
  };
}

export function mockDeleteEntries(volumeId: number, ids: number[], permanent: boolean): CleanupResultView {
  groups ??= sample();
  const mine = groups.filter((g) => g.volumeId === volumeId);
  if (mine.some((g) => g.permanent) && !permanent) {
    throw { code: 'needs_permanent', message: `volume ${volumeId}` };
  }
  let done = 0;
  let freed = 0;
  for (const g of mine) {
    const keep = g.items.filter((i) => !ids.includes(i.id));
    for (const i of g.items) {
      if (ids.includes(i.id)) {
        done++;
        freed += i.size;
      }
    }
    g.items = keep;
    g.count = keep.length;
    g.total = keep.reduce((a, i) => a + i.size, 0);
  }
  return { done, skipped: 0, failed: 0, freed };
}

/** Restores the sample proposals (tests). */
export function resetMockCleanup(): void {
  groups = null;
}
