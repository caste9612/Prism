-- Schema version 5 (T6): content hashes (F06, ADR-0011).
-- A hash is valid only while the entry still has the size and modification time it was computed
-- for: queries join on both, so a changed file never matches a stale hash (F06-AC3). Hashes stay
-- when the volume goes offline (F06-AC6) and go with their entry.
CREATE TABLE content_hashes (
  entry_id    INTEGER PRIMARY KEY REFERENCES entries (id) ON DELETE CASCADE,
  size        INTEGER NOT NULL,
  modified_at INTEGER,
  quick       BLOB,              -- BLAKE3 of the size, the first and the last 64 KiB
  full        BLOB,              -- BLAKE3 of the whole content
  computed_at INTEGER NOT NULL   -- milliseconds since 1970
) STRICT;
CREATE INDEX content_hashes_full ON content_hashes (size, full) WHERE full IS NOT NULL;
CREATE INDEX content_hashes_quick ON content_hashes (size, quick) WHERE quick IS NOT NULL;
