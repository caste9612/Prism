# Changelog

All notable changes to Prism are documented here.

## [Unreleased]

### Added
- **Storage Explorer V2**: Complete redesign with vertical disk cards and nested treemap
- **Pre-computed folder sizes**: `folder_sizes` table for instant treemap loading (~50ms vs 15s)
- **Scan timer**: Elapsed time display in progress bar during scans
- **Right-click to Explorer**: Open any folder in Windows Explorer from treemap
- **FTS5 tokenization fix**: Search now works correctly with dots, dashes, underscores in filenames

### Changed
- Drives no longer auto-selected at startup (auto-scan handles all drives)
- Treemap shows cached data immediately while refreshing in background
- Export logs now finds files with date suffix (e.g., `prism.log.2026-01-06`)

### Fixed
- Quick search now finds files like "readme.txt" or "my-file.doc" correctly
- Loading overlay no longer shows when treemap data is cached
- Drill down works on any folder click, not just top-level

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
