-- Schema version 4 (T4): log of the file operations started by Prism (F16-AC6, ADR-0012).
-- No foreign key on volume_id: the log outlives a volume removed from the catalog. The full
-- path is stored because the entry is gone after a deletion.
CREATE TABLE actions_log (
  id        INTEGER PRIMARY KEY,
  at        INTEGER NOT NULL,   -- milliseconds since 1970
  action    TEXT    NOT NULL,   -- recycle | delete
  volume_id INTEGER,
  path      TEXT    NOT NULL,
  size      INTEGER NOT NULL,
  outcome   TEXT    NOT NULL,   -- done | skipped_changed | skipped_missing | failed
  detail    TEXT
) STRICT;
CREATE INDEX actions_log_at ON actions_log (at);
