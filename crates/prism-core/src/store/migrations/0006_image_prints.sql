-- Schema version 6 (T8): perceptual prints of the images (F11).
-- Like the content hashes, a print is valid only while the entry keeps the size and
-- modification time it was computed for (queries join on both). `dhash` is NULL when the file
-- could not be decoded: it is not tried again until it changes.
CREATE TABLE image_prints (
  entry_id    INTEGER PRIMARY KEY REFERENCES entries (id) ON DELETE CASCADE,
  size        INTEGER NOT NULL,
  modified_at INTEGER,
  dhash       INTEGER,           -- 64-bit difference hash, as a signed integer
  width       INTEGER,
  height      INTEGER,
  computed_at INTEGER NOT NULL   -- milliseconds since 1970
) STRICT;
