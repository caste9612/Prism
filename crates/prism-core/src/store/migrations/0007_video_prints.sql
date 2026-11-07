-- Schema version 7 (T8): prints of the videos (F12, ADR-0013).
-- Valid while the entry keeps the size and modification time they were computed for, like
-- the content hashes and the image prints. `frames` holds the 64-bit print of each sampled
-- frame (little-endian, 8 bytes each); NULL when the video could not be decoded.
CREATE TABLE video_prints (
  entry_id    INTEGER PRIMARY KEY REFERENCES entries (id) ON DELETE CASCADE,
  size        INTEGER NOT NULL,
  modified_at INTEGER,
  duration_ms INTEGER,
  width       INTEGER,
  height      INTEGER,
  frames      BLOB,
  computed_at INTEGER NOT NULL   -- milliseconds since 1970
) STRICT;
