# Changelog

All notable changes to Prism are documented here.

## [Unreleased]

### Added
- **Queue-based scanning architecture**: Decoupled scanning and indexing with work queues
  - Scan Worker: Processes drives from scan queue sequentially
  - State Broadcaster: Emits real-time system state to frontend
  - Global state tracking for all drive statuses and progress
- **Drive Monitor**: Background monitoring for drive status changes
  - Detects when drives come online/offline
  - Auto-queues known drives for incremental scan when reconnected
  - Emits drive-status-change events to frontend
- **Selective auto-scan**: Only scans previously indexed drives at startup
  - New drives are NOT auto-scanned (user must manually scan)
  - Prevents unexpected long scans when new drives are connected
- **Duplicates page redesign**: Complete rewrite with progress tracking
  - Real-time progress during duplicate detection
  - Batch actions for selecting/deleting duplicates
  - Improved UI with file type icons and size display
- **Network drive optimization**: 10x faster detection with parallel timeout checks
  - 2-second timeout per network drive (was 15-20 seconds)
  - Parallel checking of all network drives simultaneously
  - Detection time reduced from ~20s to ~3ms
- **Storage Explorer V2 improvements**:
  - Squarified treemap algorithm for better aspect ratios
  - Shows folder contents directly instead of single drive rectangle
  - Improved color brightness for deeper levels
- **Pre-computed folder sizes**: `folder_sizes` table for instant treemap loading (~50ms vs 15s)
- **Scan timer**: Elapsed time display in progress bar during scans
- **Right-click to Explorer**: Open any folder in Windows Explorer from treemap
- **FTS5 tokenization fix**: Search now works correctly with dots, dashes, underscores in filenames

### Changed
- Drives no longer auto-selected at startup (auto-scan handles all drives)
- Treemap shows cached data immediately while refreshing in background
- Export logs now finds files with date suffix (e.g., `prism.log.2026-01-06`)
- Offline drives no longer show "Scanning" status during scan

### Fixed
- Quick search now finds files like "readme.txt" or "my-file.doc" correctly
- Loading overlay no longer shows when treemap data is cached
- Drill down works on any folder click, not just top-level
- Drive monitor lifetime error with tokio::spawn fixed

## [0.1.0] - 2026-01-05

### Added
- **Unified Quick Search**: Single search bar with extension filter chips
- **File logging**: Logs written to `%LOCALAPPDATA%\Prism\logs\`
- **Export logs**: Settings option to export all log files
- **Parallel scanning**: All drives scanned simultaneously
- **Network drive optimization**: 9x faster with walkdir

### Changed
- Removed separate local/network scan phases
- Simplified incremental scan progress UI

### Fixed
- Intermediate file batches now use batch update correctly
- Removed dead code from scanner

## [0.0.1] - 2026-01-01

### Added
- Initial release
- Auto-scan all drives at startup
- FTS5-powered quick search
- Duplicate file detection (exact + similar images)
- Analytics dashboard with charts
- System tray integration
- Global hotkey (Ctrl+Space)

---

## Version History Summary

| Version | Date | Highlights |
|---------|------|------------|
| 0.1.0 | 2026-01-05 | Unified search, parallel scanning, file logging |
| 0.0.1 | 2026-01-01 | Initial release |
