-- Schema version 11: what kind of physical disk each one is (REVIEW D65), "model|serial" as in
-- disk_health, so a disk in the drawer still says whether it is an SSD or a hard disk.
CREATE TABLE disk_media (
  disk_key TEXT    NOT NULL PRIMARY KEY,   -- "model|serial"
  media    TEXT    NOT NULL CHECK (media IN ('ssd', 'hdd')),
  rpm      INTEGER                         -- hard disks, when the disk says it
) STRICT;
