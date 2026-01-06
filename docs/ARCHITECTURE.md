# Prism Architecture

## Overview

Prism is a desktop application built with the Tauri framework, combining a Rust backend with a SvelteKit frontend. The architecture prioritizes performance and responsiveness.

## Directory Structure

```
prism/
├── src/                          # Frontend (SvelteKit + TypeScript)
│   ├── routes/                   # Page components
│   │   ├── +layout.svelte        # Root layout
│   │   ├── +page.svelte          # Main dashboard (single-page app)
│   │   └── search/               # Quick search overlay window
│   ├── lib/
│   │   ├── stores/               # Svelte state management
│   │   │   ├── stats.ts          # Application statistics
│   │   │   ├── search.ts         # Search state
│   │   │   ├── duplicates.ts     # Duplicate detection state
│   │   │   ├── analytics.ts      # Analytics data
│   │   │   └── settings.ts       # User preferences
│   │   └── utils/
│   │       └── format.ts         # Formatting utilities
│   ├── app.html
│   └── app.css
│
├── src-tauri/                    # Backend (Rust)
│   ├── src/
│   │   ├── main.rs               # Entry point
│   │   ├── lib.rs                # Tauri setup, tray, shortcuts
│   │   ├── commands/mod.rs       # IPC command handlers
│   │   ├── database/             # SQLite operations
│   │   │   ├── mod.rs            # Database wrapper
│   │   │   └── schema.rs         # Table definitions
│   │   ├── scanner/              # Filesystem scanning
│   │   │   ├── mod.rs            # Scanner orchestration
│   │   │   ├── walker.rs         # Directory traversal
│   │   │   └── hasher.rs         # BLAKE3 hashing
│   │   ├── duplicates/           # Duplicate detection
│   │   │   ├── mod.rs
│   │   │   ├── finder.rs         # Hash-based detection
│   │   │   └── phash.rs          # Perceptual image hashing
│   │   ├── search/               # Search engine
│   │   │   ├── mod.rs
│   │   │   ├── parser.rs         # Query parser
│   │   │   ├── executor.rs       # Query execution
│   │   │   └── cache.rs          # Result caching
│   │   └── utils/
│   │       ├── mod.rs
│   │       ├── format.rs         # Size formatting
│   │       └── progress.rs       # Progress reporting
│   └── Cargo.toml
│
├── docs/                         # Documentation
└── static/                       # Static assets
```

## Data Flow

### Scanning

```
┌─────────────┐    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│   Frontend  │───>│  Command    │───>│   Scanner   │───>│  Database   │
│  (Svelte)   │<───│  Handler    │<───│   (Rust)    │<───│  (SQLite)   │
└─────────────┘    └─────────────┘    └─────────────┘    └─────────────┘
      │                  │                   │
      │            Tauri Events         Progress
      │<─────────────────┴───────────────────┘
```

1. Frontend calls `start_scan` or `auto_scan_drives` command
2. Backend spawns async task for scanning
3. Scanner walks filesystem using producer-consumer pattern
4. Files batched (1000/batch) and inserted into SQLite
5. Progress emitted via Tauri events (`scan-progress`)
6. Frontend updates UI reactively

### Search

```
┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│  Search     │───>│   FTS5      │───>│   Results   │
│  Query      │    │   Engine    │    │   (JSON)    │
└─────────────┘    └─────────────┘    └─────────────┘
```

1. Query parsed for filters (size:, ext:, type:, path:)
2. FTS5 MATCH query with BM25 ranking
3. Results limited and returned as JSON

## Database Schema

```sql
-- Core file metadata
CREATE TABLE files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    extension TEXT,
    size INTEGER NOT NULL,
    created_at INTEGER,
    modified_at INTEGER,
    accessed_at INTEGER,
    partial_hash BLOB,          -- First 64KB BLAKE3
    full_hash BLOB,             -- Full file BLAKE3
    perceptual_hash INTEGER,    -- dHash for images
    scan_id INTEGER NOT NULL
);

-- Full-text search
CREATE VIRTUAL TABLE files_fts USING fts5(
    name, path, extension,
    content='files',
    content_rowid='id'
);

-- Pre-computed folder sizes (for fast treemap)
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

-- Scan history
CREATE TABLE scans (
    id INTEGER PRIMARY KEY,
    started_at INTEGER NOT NULL,
    completed_at INTEGER,
    root_paths TEXT NOT NULL,   -- JSON array
    total_files INTEGER,
    total_size INTEGER,
    status TEXT DEFAULT 'running'
);

-- Indexes
CREATE INDEX idx_files_extension ON files(extension);
CREATE INDEX idx_files_size ON files(size);
CREATE INDEX idx_files_partial_hash ON files(partial_hash);
CREATE INDEX idx_files_phash ON files(perceptual_hash);
CREATE INDEX idx_folder_sizes_drive ON folder_sizes(drive);
CREATE INDEX idx_folder_sizes_parent ON folder_sizes(parent_path);
```

## Key Components

### Scanner (`scanner/`)

- **Walker**: Uses `walkdir` crate for fast directory traversal
- **Producer-Consumer**: 8 parallel consumers for network drives
- **Batching**: 1000 files per database transaction
- **Hasher**: BLAKE3 for partial (64KB) and full file hashing

### Search Engine (`search/`)

- **Parser**: Regex-based filter extraction (size:, ext:, type:)
- **Executor**: SQLite FTS5 with BM25 ranking
- **Cache**: LRU cache for frequent queries

### Duplicate Detection (`duplicates/`)

- **Two-Phase**: Size + partial hash pre-filter, full hash confirmation
- **Perceptual**: dHash (difference hash) for image similarity
- **Threshold**: Configurable Hamming distance for similar images

## Communication

### Tauri Commands (IPC)

Frontend communicates via `invoke()`:
```typescript
const stats = await invoke<AppStats>('get_stats');
const results = await invoke<SearchResponse>('quick_search', { query, limit });
```

### Events

Backend emits progress events:
```rust
app_handle.emit("scan-progress", progress)?;
app_handle.emit("scan-complete", ())?;
```

Frontend listens:
```typescript
await listen<ScanProgress>('scan-progress', (event) => {
    scanProgress = event.payload;
});
```

## Performance Optimizations

1. **Parallel Scanning**: Rayon for CPU-bound work, async for I/O
2. **Batched Inserts**: 1000 files per transaction
3. **FTS5**: Full-text search with BM25 ranking
4. **Lazy Hashing**: Full hash computed only for duplicate candidates
5. **Event-Based UI**: No polling, reactive updates
6. **SQLite WAL**: Write-ahead logging for concurrent reads

## Window Management

- **Main Window**: Dashboard with tabs (Analytics, Duplicates, Settings)
- **Search Window**: Overlay spawned via global shortcut (Ctrl+Space)
- **System Tray**: Minimize to tray, quick access menu
