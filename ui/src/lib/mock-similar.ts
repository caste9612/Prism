// Simulated similar images (browser development, tests), with the shapes of the backend: groups
// within a distance, the largest image first, thumbnails drawn as coloured placeholders.
import type { CleanupResultView } from './bindings/CleanupResultView';
import type { SimilarDeletion } from './bindings/SimilarDeletion';
import type { SimilarFileView } from './bindings/SimilarFileView';
import type { SimilarView } from './bindings/SimilarView';
import type { VolumeView } from './bindings/VolumeView';

const MB = 1_000_000;

// [entryId, volumeId, path, width, height, size, distance from the first of its group]
type Sample = [number, number, string, number, number, number, number];

const GROUPS: Sample[][] = [
  [
    [801, 2, 'Foto\\2019\\Mare\\IMG_2041.CR2', 6000, 4000, 25 * MB, 0],
    [802, 2, 'Foto\\Esportate\\mare-2041.jpg', 2048, 1365, 1.9 * MB, 3],
    [803, 3, 'Foto\\2019\\IMG_2041.jpg', 6000, 4000, 7.8 * MB, 1],
  ],
  [
    [811, 1, 'Users\\me\\Pictures\\logo.png', 1024, 1024, 0.4 * MB, 0],
    [812, 1, 'Users\\me\\Desktop\\logo piccolo.png', 256, 256, 0.05 * MB, 5],
  ],
  [
    [821, 4, 'Archivio\\scansioni\\ricevuta.tif', 2480, 3508, 9 * MB, 0],
    [822, 2, 'Documenti\\ricevuta.jpg', 1240, 1754, 0.6 * MB, 9],
  ],
];

const VIDEO_GROUPS: Sample[][] = [
  [
    [901, 2, 'Video\\Vacanze 2019.mp4', 1920, 1080, 4.2 * 1000 * MB, 0],
    [902, 4, 'Archivio\\Video\\Vacanze 2019 (720p).mkv', 1280, 720, 1.1 * 1000 * MB, 1],
  ],
];
const DURATIONS: Record<number, number> = { 901: 754_000, 902: 755_200 };

export function mockSimilarVideos(
  volumes: VolumeView[],
  maxDistance: number,
  volumeId: number | null,
  deleted: Set<number> = new Set(),
): SimilarView {
  const view = build(VIDEO_GROUPS, volumes, maxDistance, volumeId, deleted);
  view.groups.forEach((g) => g.files.forEach((f) => (f.durationMs = DURATIONS[f.entryId] ?? null)));
  return { ...view, pending: 42 };
}

export function mockSimilar(
  volumes: VolumeView[],
  maxDistance: number,
  volumeId: number | null,
  deleted: Set<number> = new Set(),
): SimilarView {
  return build(GROUPS, volumes, maxDistance, volumeId, deleted);
}

/** Deletes files of groups of similar files as the backend does (REVIEW D67). */
export function mockDeleteSimilar(
  volumes: VolumeView[],
  groups: SimilarDeletion[],
  permanent: boolean,
  deleted: Set<number>,
): CleanupResultView {
  const samples = new Map([...GROUPS, ...VIDEO_GROUPS].flat().map((s) => [s[0], s]));
  let freed = 0;
  let done = 0;
  for (const g of groups) {
    if (g.delete.length === 0) continue;
    if (g.keep.length === 0 || g.delete.some((d) => g.keep.includes(d))) {
      throw { code: 'keep_one', message: String(g.delete[0]) };
    }
    for (const id of g.delete) {
      const s = samples.get(id);
      if (!s) throw { code: 'not_found', message: String(id) };
      const vol = volumes.find((v) => v.id === s[1]);
      if (!vol?.online) throw { code: 'volume_offline', message: String(s[1]) };
      if (vol.kind !== 'internal' && !permanent) throw { code: 'needs_permanent', message: String(s[1]) };
      freed += s[5];
      done += 1;
    }
  }
  groups.forEach((g) => g.delete.forEach((id) => deleted.add(id)));
  return { done, skipped: 0, failed: 0, freed };
}

function build(
  all: Sample[][],
  volumes: VolumeView[],
  maxDistance: number,
  volumeId: number | null,
  deleted: Set<number>,
): SimilarView {
  const byId = new Map(volumes.map((v) => [v.id, v]));
  const source = all.map((g) => g.filter(([id]) => !deleted.has(id)));
  const groups = source.filter((g) => g.filter(([, , , , , , d]) => d <= maxDistance).length > 1)
    .filter((g) => volumeId === null || g.some(([, v]) => v === volumeId))
    .map((g) => {
      const files: SimilarFileView[] = g
        .filter(([, , , , , , d]) => d <= maxDistance)
        .map(([entryId, volId, path, width, height, size, distance]) => {
          const v = byId.get(volId);
          const mount = (v?.mount ?? '').replace(/\\$/, '');
          return {
            entryId,
            volumeId: volId,
            disk: v?.name ?? `#${volId}`,
            location: v?.location ?? null,
            path,
            fullPath: mount ? `${mount}\\${path}` : path,
            width,
            height,
            size,
            durationMs: null,
            online: v?.online ?? false,
            permanent: v?.kind !== 'internal',
            distance,
          };
        });
      return { key: String(files[0]?.entryId ?? 0), files, count: files.length };
    });
  return { groups, totalGroups: groups.length, pending: 3_210 };
}

/** A placeholder thumbnail: a gradient whose colour follows the group. */
export function mockThumbnail(entryId: number): string {
  const hue = (Math.floor(entryId / 10) * 67) % 360;
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="192" height="128">` +
    `<defs><linearGradient id="g"><stop offset="0" stop-color="hsl(${hue},60%,45%)"/>` +
    `<stop offset="1" stop-color="hsl(${(hue + 40) % 360},70%,70%)"/></linearGradient></defs>` +
    `<rect width="192" height="128" fill="url(#g)"/><circle cx="60" cy="56" r="26" fill="hsl(${hue},40%,85%)"/></svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}
