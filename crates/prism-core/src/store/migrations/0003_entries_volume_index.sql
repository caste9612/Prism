-- Schema version 3 (T2): index entries by volume only.
-- The index on (volume_id, file_ref) made every insert of an MFT scan land on a random index
-- page: 1 million inserts took 13.0 s instead of 2.4 s (measured, ADR-0017). Lookups by file
-- reference use an in-memory map built from the volume's entries when needed (0.18 s per
-- million entries).
DROP INDEX entries_volume_ref;
CREATE INDEX entries_volume ON entries (volume_id);
