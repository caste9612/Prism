# Prism Features

## Overview

Prism is a high-performance disk analyzer for Windows that scans your drives, indexes files, and helps you understand disk usage through visual treemaps, quick search, and duplicate detection.

## Core Features

### 1. Auto-Scan at Startup

When Prism starts, it automatically scans all available drives:
- **Local drives**: C:\, D:\, E:\, etc.
- **Network drives**: Mapped network shares (X:\, Y:\, Z:\)
- **Incremental scanning**: Only processes changed files on subsequent runs

The scan uses parallel processing for maximum speed:
- All drives scanned simultaneously
- ~25,000-30,000 files/second on local SSDs
- ~500-1,000 files/second on network drives

### 2. Quick Search (Ctrl+Space)

Global hotkey launches instant file search from anywhere:

**Search Syntax:**
- `filename` - Search by name
- `ext:pdf,docx` - Filter by extension
- `size:>1GB` - Files larger than 1GB
- `size:<100KB` - Files smaller than 100KB
- `size:1MB-10MB` - Files in size range
- `type:image` - All image files (jpg, png, gif, etc.)
- `type:video` - All video files (mp4, avi, mkv, etc.)
- `type:document` - All documents (pdf, docx, xlsx, etc.)
- `path:Downloads` - Files in path containing "Downloads"

**Type Categories:**
| Category | Extensions |
|----------|------------|
| image | jpg, jpeg, png, gif, bmp, webp, svg, ico, tiff |
| video | mp4, avi, mkv, mov, wmv, flv, webm |
| audio | mp3, wav, flac, aac, ogg, wma, m4a |
| document | pdf, doc, docx, xls, xlsx, ppt, pptx, txt, rtf |
| archive | zip, rar, 7z, tar, gz, bz2 |
| code | js, ts, py, java, c, cpp, rs, go, html, css |
| executable | exe, msi, dll, bat, cmd, ps1 |

### 3. Storage Explorer (Treemap)

Visual disk usage analysis with interactive treemap:

**All Drives View:**
- Vertical card layout showing all drives
- Usage bars with color coding (green < 75%, yellow < 90%, red > 90%)
- Shows used/free/total space per drive
- File count per indexed drive

**Treemap View:**
- Squarified treemap algorithm for optimal rectangle layout
- Multi-level nested visualization (folders within folders)
- Color-coded by folder
- Click to drill down into subfolders
- Right-click to open folder in Windows Explorer

**Performance:**
- Pre-computed folder sizes table
- Single query + in-memory tree building
- ~50ms load time for entire drive treemap

### 4. Duplicate Detection

Find exact and similar duplicate files:

**Exact Duplicates:**
- Two-phase detection: size + partial hash, then full hash
- BLAKE3 hashing (fastest cryptographic hash)
- Partial hash = first 64KB (fast pre-filter)
- Groups files by content, shows wasted space

**Similar Images:**
- Perceptual hashing (dHash algorithm)
- Finds visually similar images even if resized/recompressed
- Configurable similarity threshold
- Hamming distance comparison

**Actions:**
- Preview files before deletion
- Delete individual files or batch delete
- Recycle bin support (safe deletion)

### 5. Analytics Dashboard

**Statistics Overview:**
- Total files indexed
- Total disk space analyzed
- Duplicate count and wasted space
- Last scan timestamp

**File Distribution Charts:**
- By extension (top 20)
- By file type category
- By size range

**Drive Statistics:**
- Per-drive file count
- Per-drive size breakdown
- Extension distribution per drive

### 6. Scan Progress UI

Real-time feedback during scanning:

**Phase Indicators:**
1. **Prepare** - Loading existing database
2. **Scan** - Walking filesystem
3. **Clean** - Removing deleted files
4. **Index** - Rebuilding FTS5 search index
5. **Done** - Scan complete

**Progress Information:**
- Elapsed time timer
- Files checked count
- New/Updated/Deleted file counts
- Per-drive progress percentage
- Files per second rate

### 7. System Integration

**System Tray:**
- Minimize to tray (close button)
- Tray icon with context menu
- Quick access to search and quit

**Global Hotkey:**
- Ctrl+Space opens Quick Search from anywhere
- Works even when Prism is minimized

**Windows Explorer Integration:**
- Right-click any folder in treemap to open in Explorer
- Export results to CSV/JSON

### 8. Logging & Diagnostics

**File Logging:**
- Logs stored in `%LOCALAPPDATA%\Prism\logs\`
- Daily rotation: `prism.log.YYYY-MM-DD`
- Configurable log level

**Export Logs:**
- Settings > Export Logs
- Copies all log files to chosen directory
- Useful for debugging issues

## Database Schema

### Files Table
```sql
CREATE TABLE files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    extension TEXT,
    size INTEGER NOT NULL,
    created_at INTEGER,
    modified_at INTEGER,
    accessed_at INTEGER,
    partial_hash BLOB,      -- First 64KB BLAKE3
    full_hash BLOB,         -- Full file BLAKE3
    perceptual_hash INTEGER, -- dHash for images
    scan_id INTEGER NOT NULL
);
```

### FTS5 Search Index
```sql
CREATE VIRTUAL TABLE files_fts USING fts5(
    name, path, extension,
    content='files',
    content_rowid='id'
);
```

### Folder Sizes (Pre-computed)
```sql
CREATE TABLE folder_sizes (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    drive TEXT NOT NULL,
    depth INTEGER NOT NULL,
    total_size INTEGER NOT NULL,
    file_count INTEGER NOT NULL,
    folder_count INTEGER NOT NULL,
    parent_path TEXT,
    scan_id INTEGER NOT NULL
);
```

## Performance Characteristics

| Operation | Time | Notes |
|-----------|------|-------|
| Local drive scan | ~20s for 500K files | Parallel, incremental |
| Network drive scan | ~5-10min for 500K files | Depends on latency |
| Quick search | <10ms | FTS5 with BM25 ranking |
| Treemap load | ~50ms | Pre-computed aggregates |
| Duplicate detection | ~30s for 2M files | Two-phase with hash caching |
| FTS rebuild | ~20s for 2M files | Batch inserts, WAL mode |

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| Ctrl+Space | Open Quick Search |
| Escape | Close Quick Search / Cancel |
| Enter | Open selected file |
| Ctrl+Enter | Open file location |

## Settings

**Scan Settings:**
- Exclude hidden files
- Exclude system files
- Exclude patterns (glob)
- Minimum file size filter

**UI Settings:**
- Theme (dark mode default)
- Startup behavior
- Tray icon options
