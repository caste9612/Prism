-- Prism catalog, schema version 1 (docs/ARCHITECTURE.md §3, ADR-0005).
-- Integrity between tables is enforced with foreign keys (the writer connection enables them).

CREATE TABLE volumes (
    id              INTEGER PRIMARY KEY,
    kind            TEXT    NOT NULL CHECK (kind IN ('fixed', 'removable', 'network')),
    fs_type         TEXT,
    fs_serial       INTEGER,
    volume_guid     TEXT,
    unc_path        TEXT,
    os_label        TEXT,
    capacity        INTEGER,
    free_space      INTEGER,
    -- user labels (F03)
    user_name       TEXT,
    location        TEXT,
    notes           TEXT,
    color           TEXT,
    -- state
    status          TEXT    NOT NULL
                    CHECK (status IN ('new', 'indexing', 'indexed', 'ignored', 'reindex_needed')),
    last_mount      TEXT,
    first_seen_at   INTEGER NOT NULL,  -- Unix milliseconds
    last_seen_at    INTEGER,
    last_indexed_at INTEGER,
    -- USN journal position (NTFS only, T2)
    usn_journal_id  INTEGER,
    usn_next        INTEGER,
    settings        TEXT               -- JSON: per-volume exclusions, hash policy
) STRICT;

-- One row per file or folder. The full path is not stored: it is rebuilt from parent_id.
CREATE TABLE entries (
    id          INTEGER PRIMARY KEY,
    volume_id   INTEGER NOT NULL REFERENCES volumes (id),
    parent_id   INTEGER REFERENCES entries (id),   -- NULL only for the root of a volume
    name        TEXT    NOT NULL,                   -- stored name (names.rs); '' for the root
    is_dir      INTEGER NOT NULL,
    size        INTEGER NOT NULL,                   -- logical size, 0 for folders
    alloc_size  INTEGER,                            -- allocated size, when known
    created_at  INTEGER,                            -- FILETIME (100 ns since 1601)
    modified_at INTEGER,                            -- FILETIME
    attrs       INTEGER NOT NULL,                   -- FILE_ATTRIBUTE_*
    file_ref    INTEGER                             -- NTFS file reference, when known
) STRICT;

-- Children of a folder, by exact name (names are unique per folder, compared exactly).
CREATE UNIQUE INDEX entries_parent_name ON entries (parent_id, name);
-- Entries of a volume and lookup by file reference (moves, USN journal).
CREATE INDEX entries_volume_ref ON entries (volume_id, file_ref);

-- Aggregates of each folder over its whole subtree (F04-AC4).
CREATE TABLE dir_stats (
    entry_id    INTEGER PRIMARY KEY REFERENCES entries (id) ON DELETE CASCADE,
    total_size  INTEGER NOT NULL,
    total_alloc INTEGER NOT NULL,
    file_count  INTEGER NOT NULL,
    dir_count   INTEGER NOT NULL
) STRICT;

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;
