// Simulated duplicates (browser development, tests), with the shapes and the rules of the
// backend (prism-core::duplicates): filters, selection rules, last-copy rule on deletion.
import type { CleanupResultView } from './bindings/CleanupResultView';
import type { DupFileView } from './bindings/DupFileView';
import type { DupFilterInput } from './bindings/DupFilterInput';
import type { DupGroupView } from './bindings/DupGroupView';
import type { DuplicatesView } from './bindings/DuplicatesView';
import type { RuleInput } from './bindings/RuleInput';
import type { VolumeView } from './bindings/VolumeView';

const MB = 1_000_000;
const GB = 1_000_000_000;
const DAY = 86_400_000;

type Copy = [entryId: number, volumeId: number, path: string, ageDays: number];
type Sample = { size: number; kind: string; copies: Copy[] };

const SAMPLES: Sample[] = [
  {
    size: 4.2 * GB,
    kind: 'videos',
    copies: [
      [101, 2, 'Video\\Vacanze 2019.mp4', 900],
      [102, 3, 'Video\\Vacanze 2019.mp4', 880],
      [103, 4, 'Archivio\\Video\\Vacanze 2019.mp4', 400],
    ],
  },
  {
    size: 700 * MB,
    kind: 'disk_images',
    copies: [
      [201, 1, 'Users\\me\\Downloads\\setup-tools.iso', 20],
      [202, 2, 'ISO\\setup-tools.iso', 300],
    ],
  },
  {
    size: 300 * MB,
    kind: 'archives',
    copies: [
      [301, 3, 'Archivio\\Album.zip', 1200],
      [302, 3, 'Vecchio\\Album.zip', 1500],
    ],
  },
  {
    size: 6.1 * MB,
    kind: 'images',
    copies: [
      [401, 2, 'Foto\\2019\\IMG_2041.jpg', 950],
      [402, 2, 'Foto\\Da ordinare\\IMG_2041.jpg', 120],
      [403, 3, 'Foto\\2019\\IMG_2041.jpg', 940],
    ],
  },
  {
    size: 2.4 * MB,
    kind: 'documents',
    copies: [
      [501, 1, 'Users\\me\\Documents\\Relazione finale.pdf', 60],
      [502, 1, 'Users\\me\\Desktop\\Relazione finale (copia).pdf', 15],
    ],
  },
];

const fold = (s: string) => s.toLowerCase();

/** The selection rules of prism-core::duplicates::select. */
function select(files: DupFileView[], rules: RuleInput[]): number[] {
  let keepers = [...files];
  for (const r of rules) {
    let narrowed: DupFileView[] = [];
    if (r.kind === 'keepOnVolume') narrowed = keepers.filter((f) => f.volumeId === r.volumeId);
    else if (r.kind === 'keepOldest' || r.kind === 'keepNewest') {
      const times = keepers.map((f) => f.modified ?? NaN).filter((m) => !Number.isNaN(m));
      const pick = r.kind === 'keepOldest' ? Math.min(...times) : Math.max(...times);
      narrowed = keepers.filter((f) => f.modified === pick);
    } else if (r.kind === 'keepShortestPath') {
      const min = Math.min(...keepers.map((f) => f.path.length));
      narrowed = keepers.filter((f) => f.path.length === min);
    } else continue;
    if (narrowed.length > 0) keepers = narrowed;
  }
  const keeper = Math.min(...keepers.map((f) => f.entryId));
  const protectedTexts = rules
    .filter((r) => r.kind === 'neverIn' && (r.text ?? '').trim() !== '')
    .map((r) => fold((r.text ?? '').trim()));
  return files
    .filter((f) => f.entryId !== keeper && f.online)
    .filter((f) => !protectedTexts.some((p) => fold(f.path).includes(p)))
    .map((f) => f.entryId);
}

export function mockDuplicates(
  volumes: VolumeView[],
  filter: DupFilterInput,
  rules: RuleInput[],
  deleted: Set<number>,
  now = Date.UTC(2026, 10, 7),
): DuplicatesView {
  const byId = new Map(volumes.map((v) => [v.id, v]));
  const folder = fold((filter.folder ?? '').trim());
  const groups: DupGroupView[] = [];
  for (const s of SAMPLES) {
    const files: DupFileView[] = s.copies
      .filter(([id]) => !deleted.has(id))
      .map(([entryId, volumeId, path, age]) => {
        const v = byId.get(volumeId);
        const mount = (v?.mount ?? '').replace(/\\$/, '');
        return {
          entryId,
          volumeId,
          disk: v?.name ?? `#${volumeId}`,
          location: v?.location ?? null,
          path,
          fullPath: mount ? `${mount}\\${path}` : path,
          modified: now - age * DAY,
          online: v?.online ?? false,
          permanent: v ? v.kind !== 'internal' : true,
        };
      });
    if (files.length < 2 || s.size < filter.minSize) continue;
    if (filter.category && filter.category !== s.kind) continue;
    const matches = files.some(
      (f) => (filter.volumeId === null || f.volumeId === filter.volumeId) && (!folder || fold(f.path).includes(folder)),
    );
    if (!matches) continue;
    groups.push({
      key: `${s.size}-${s.copies[0]?.[0] ?? 0}`,
      size: s.size,
      recoverable: s.size * (files.length - 1),
      kind: s.kind,
      files,
      selected: select(files, rules),
    });
  }
  groups.sort((a, b) => b.recoverable - a.recoverable);
  return {
    groups,
    totalGroups: groups.length,
    totalRecoverable: groups.reduce((a, g) => a + g.recoverable, 0),
    pendingFiles: 12_480,
  };
}

/** The deletion with the last-copy rule (F16-AC4); the files are only marked as gone. */
export function mockDeleteDuplicates(
  volumes: VolumeView[],
  ids: number[],
  allowOfflineOnly: boolean,
  permanent: boolean,
  deleted: Set<number>,
): CleanupResultView {
  const online = new Set(volumes.filter((v) => v.online).map((v) => v.id));
  const doomed = new Set(ids);
  let freed = 0;
  for (const s of SAMPLES) {
    const live = s.copies.filter(([id]) => !deleted.has(id));
    const left = live.filter(([id]) => !doomed.has(id));
    const going = live.filter(([id]) => doomed.has(id));
    if (going.length === 0) continue;
    const first = String(going[0]?.[0] ?? '');
    if (left.length === 0) throw { code: 'last_copy', message: first };
    if (!allowOfflineOnly && !left.some(([, v]) => online.has(v))) {
      throw { code: 'only_offline_copies', message: first };
    }
    for (const [, v] of going) {
      if (!online.has(v)) throw { code: 'volume_offline', message: String(v) };
      const vol = volumes.find((x) => x.id === v);
      if (vol && vol.kind !== 'internal' && !permanent) throw { code: 'needs_permanent', message: String(v) };
    }
    freed += going.length * s.size;
  }
  ids.forEach((id) => deleted.add(id));
  return { done: ids.length, skipped: 0, failed: 0, freed };
}
