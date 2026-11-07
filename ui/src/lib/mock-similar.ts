// Simulated similar images (browser development, tests), with the shapes of the backend: groups
// within a distance, the largest image first, thumbnails drawn as coloured placeholders.
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

export function mockSimilar(volumes: VolumeView[], maxDistance: number, volumeId: number | null): SimilarView {
  const byId = new Map(volumes.map((v) => [v.id, v]));
  const groups = GROUPS.filter((g) => g.filter(([, , , , , , d]) => d <= maxDistance).length > 1)
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
            online: v?.online ?? false,
            distance,
          };
        });
      return { key: String(files[0]?.entryId ?? 0), files };
    });
  return { groups, totalGroups: groups.length, pendingImages: 3_210 };
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
