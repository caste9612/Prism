-- Schema version 8 (T9): history of the health of the physical disks (F17-AC2).
-- Disks are identified by model and serial number (disk numbers change across reboots); a
-- reading is kept when something changed or once a day.
CREATE TABLE disk_health (
  disk_key       TEXT    NOT NULL,   -- "model|serial"
  at             INTEGER NOT NULL,   -- milliseconds since 1970
  status         TEXT    NOT NULL CHECK (status IN ('good', 'warning', 'critical')),
  temperature    INTEGER,
  power_on_hours INTEGER,
  wear           INTEGER,
  reallocated    INTEGER,
  pending        INTEGER,
  media_errors   INTEGER,
  spare          INTEGER,
  reasons        TEXT,               -- codes separated by commas
  PRIMARY KEY (disk_key, at)
) STRICT;
