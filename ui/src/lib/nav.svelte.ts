// Where the main window is (ADR-0019): everything starts from the catalog or from a disk. There
// is no side menu; the top bar shows the path and the search box.

/** The tabs of a disk screen, in their order. */
export const DISK_TABS = ['space', 'duplicates', 'similar', 'backup', 'cleanup', 'health', 'types'] as const;
export type DiskTab = (typeof DISK_TABS)[number];

/** The functions on the whole catalog, from the "whole catalog" block. */
export const CATALOG_TABS = ['duplicates', 'similar', 'backup', 'types'] as const;
export type CatalogTab = (typeof CATALOG_TABS)[number];

export type Route =
  | { name: 'catalog' }
  | { name: 'disk'; volumeId: number; tab: DiskTab; folderId: number | null }
  | { name: 'all'; tab: CatalogTab }
  | { name: 'search'; query: string }
  | { name: 'settings' };

class Navigator {
  route = $state<Route>({ name: 'catalog' });
  /** Bumped at every navigation, so a screen opened again starts from its new parameters. */
  visit = $state(0);

  go(route: Route): void {
    this.route = route;
    this.visit += 1;
  }

  disk(volumeId: number, tab: DiskTab = 'space', folderId: number | null = null): void {
    this.go({ name: 'disk', volumeId, tab, folderId });
  }
}

export const nav = new Navigator();
