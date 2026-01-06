//! Database schema definition for Prism

/// SQL schema for initializing the database
pub const SCHEMA_SQL: &str = r#"
-- Core file metadata
CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    extension TEXT,
    size INTEGER NOT NULL,
    created_at INTEGER,
    modified_at INTEGER,
    accessed_at INTEGER,
    attributes INTEGER,
    partial_hash BLOB,
    full_hash BLOB,
    perceptual_hash BLOB,
    video_fingerprint BLOB,
    scan_id INTEGER NOT NULL,
    FOREIGN KEY (scan_id) REFERENCES scans(id)
);

-- FTS5 full-text search virtual table
CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(
    name,
    path,
    extension,
    content='files',
    content_rowid='id',
    tokenize='unicode61 remove_diacritics 2'
);

-- Triggers to keep FTS index in sync with files table
CREATE TRIGGER IF NOT EXISTS files_ai AFTER INSERT ON files BEGIN
    INSERT INTO files_fts(rowid, name, path, extension)
    VALUES (new.id, new.name, new.path, new.extension);
END;

CREATE TRIGGER IF NOT EXISTS files_ad AFTER DELETE ON files BEGIN
    INSERT INTO files_fts(files_fts, rowid, name, path, extension)
    VALUES ('delete', old.id, old.name, old.path, old.extension);
END;

CREATE TRIGGER IF NOT EXISTS files_au AFTER UPDATE ON files BEGIN
    INSERT INTO files_fts(files_fts, rowid, name, path, extension)
    VALUES ('delete', old.id, old.name, old.path, old.extension);
    INSERT INTO files_fts(rowid, name, path, extension)
    VALUES (new.id, new.name, new.path, new.extension);
END;

-- Scan sessions
CREATE TABLE IF NOT EXISTS scans (
    id INTEGER PRIMARY KEY,
    started_at INTEGER NOT NULL,
    completed_at INTEGER,
    root_paths TEXT NOT NULL,
    total_files INTEGER,
    total_size INTEGER,
    status TEXT DEFAULT 'running'
);

-- Duplicate groups
CREATE TABLE IF NOT EXISTS duplicate_groups (
    id INTEGER PRIMARY KEY,
    group_type TEXT NOT NULL,
    similarity_score REAL,
    file_count INTEGER,
    total_size INTEGER
);

-- Duplicate group members
CREATE TABLE IF NOT EXISTS duplicate_members (
    group_id INTEGER,
    file_id INTEGER,
    is_original INTEGER DEFAULT 0,
    PRIMARY KEY (group_id, file_id),
    FOREIGN KEY (group_id) REFERENCES duplicate_groups(id),
    FOREIGN KEY (file_id) REFERENCES files(id)
);

-- User preferences
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT
);

-- Known drives (for tracking offline/online status)
CREATE TABLE IF NOT EXISTS known_drives (
    path TEXT PRIMARY KEY,
    volume_name TEXT,
    drive_type TEXT NOT NULL,
    last_scan_at INTEGER NOT NULL,
    total_files INTEGER NOT NULL DEFAULT 0,
    total_size INTEGER NOT NULL DEFAULT 0
);

-- Indexes for fast queries
CREATE INDEX IF NOT EXISTS idx_files_name ON files(name);
CREATE INDEX IF NOT EXISTS idx_files_extension ON files(extension);
CREATE INDEX IF NOT EXISTS idx_files_size ON files(size);
CREATE INDEX IF NOT EXISTS idx_files_modified ON files(modified_at);
CREATE INDEX IF NOT EXISTS idx_files_partial_hash ON files(partial_hash) WHERE partial_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_files_full_hash ON files(full_hash) WHERE full_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_files_perceptual ON files(perceptual_hash) WHERE perceptual_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_files_scan_id ON files(scan_id);

-- Composite indexes for duplicate detection (critical for performance)
CREATE INDEX IF NOT EXISTS idx_files_size_partial_hash ON files(size, partial_hash) WHERE partial_hash IS NOT NULL AND size >= 1024;
CREATE INDEX IF NOT EXISTS idx_files_size_full_hash ON files(size, full_hash) WHERE full_hash IS NOT NULL;

-- Index for extension-based queries (case-insensitive)
CREATE INDEX IF NOT EXISTS idx_files_ext_lower ON files(LOWER(extension)) WHERE extension IS NOT NULL;

-- Covering index for search results (avoids table lookups)
CREATE INDEX IF NOT EXISTS idx_files_search_cover ON files(name, path, size, extension, modified_at);

-- Index for path-based prefix queries (critical for folder navigation and tree view)
-- Uses first 3 chars (drive letter) + full path for efficient LIKE prefix matching
CREATE INDEX IF NOT EXISTS idx_files_path_prefix ON files(SUBSTR(path, 1, 3), path);

-- Index for path and size (helps with folder size aggregation)
CREATE INDEX IF NOT EXISTS idx_files_path_size ON files(path, size);

-- Pre-computed folder sizes for fast treemap visualization
-- Populated during scan to avoid expensive runtime aggregation
CREATE TABLE IF NOT EXISTS folder_sizes (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,          -- Full folder path (e.g., "C:\Users\Documents")
    name TEXT NOT NULL,                 -- Folder name only (e.g., "Documents")
    drive TEXT NOT NULL,                -- Drive letter or UNC root (e.g., "C:\", "\\server\share\")
    depth INTEGER NOT NULL,             -- 0 = drive root, 1 = first level, etc.
    total_size INTEGER NOT NULL,        -- Sum of all files in this folder and subfolders
    file_count INTEGER NOT NULL,        -- Number of files in this folder and subfolders
    folder_count INTEGER NOT NULL,      -- Number of direct child folders
    parent_path TEXT,                   -- Parent folder path (NULL for drive roots)
    scan_id INTEGER NOT NULL,
    FOREIGN KEY (scan_id) REFERENCES scans(id)
);

-- Indexes for efficient treemap queries
CREATE INDEX IF NOT EXISTS idx_folder_sizes_drive ON folder_sizes(drive);
CREATE INDEX IF NOT EXISTS idx_folder_sizes_parent ON folder_sizes(parent_path);
CREATE INDEX IF NOT EXISTS idx_folder_sizes_drive_depth ON folder_sizes(drive, depth);
CREATE INDEX IF NOT EXISTS idx_folder_sizes_size ON folder_sizes(total_size DESC);
"#;
