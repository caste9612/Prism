-- Schema version 10: the physical disk of each local volume (REVIEW D64), "model|serial" as in
-- disk_health, so the partitions of one disk are shown together even when it is disconnected.
ALTER TABLE volumes ADD COLUMN disk_key TEXT;
