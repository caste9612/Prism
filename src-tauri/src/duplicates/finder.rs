//! Duplicate file finder with two-phase hashing

use crate::database::Database;
use anyhow::Result;
use blake3::Hasher;
use rayon::prelude::*;
use serde::Serialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use tracing::{debug, info, warn};

/// Minimum file size to consider for deduplication (skip tiny files)
const MIN_DUPLICATE_SIZE: i64 = 1024; // 1KB

/// A group of duplicate files
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateGroup {
    pub id: i64,
    pub hash: String,
    pub size: i64,
    pub files: Vec<DuplicateFile>,
    pub wasted_space: i64,
}

/// A file within a duplicate group
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateFile {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub extension: Option<String>,
    pub modified_at: Option<i64>,
    pub is_original: bool,
}

/// Duplicate finder with two-phase hashing
pub struct DuplicateFinder {
    min_size: i64,
}

impl Default for DuplicateFinder {
    fn default() -> Self {
        Self::new()
    }
}

impl DuplicateFinder {
    pub fn new() -> Self {
        Self {
            min_size: MIN_DUPLICATE_SIZE,
        }
    }

    /// Set minimum file size for duplicate detection
    pub fn with_min_size(mut self, size: i64) -> Self {
        self.min_size = size;
        self
    }

    /// Find all duplicate files in the database
    pub fn find_duplicates(&self, db: &Database) -> Result<Vec<DuplicateGroup>> {
        info!("Starting duplicate detection");

        // Phase 1: Find candidates by size and partial hash
        let candidates = self.find_candidates(db)?;
        info!("Found {} candidate groups", candidates.len());

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        // Phase 2: Compute full hashes for candidates and confirm duplicates
        let duplicates = self.confirm_duplicates(db, candidates)?;
        info!("Confirmed {} duplicate groups", duplicates.len());

        Ok(duplicates)
    }

    /// Phase 1: Find files with same size and partial hash
    fn find_candidates(&self, db: &Database) -> Result<Vec<CandidateGroup>> {
        let conn = db.connection();

        // Query files grouped by size and partial hash
        let mut stmt = conn.prepare(
            "SELECT size, partial_hash, COUNT(*) as cnt
             FROM files
             WHERE size >= ?1 AND partial_hash IS NOT NULL
             GROUP BY size, partial_hash
             HAVING cnt > 1
             ORDER BY size DESC",
        )?;

        let rows = stmt.query_map([self.min_size], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Vec<u8>>(1)?,
            ))
        })?;

        let mut candidates = Vec::new();
        for row in rows {
            let (size, partial_hash) = row?;
            candidates.push(CandidateGroup {
                size,
                partial_hash,
            });
        }

        debug!("Found {} candidate groups by size+partial_hash", candidates.len());
        Ok(candidates)
    }

    /// Phase 2: Compute full hashes and confirm true duplicates
    fn confirm_duplicates(
        &self,
        db: &Database,
        candidates: Vec<CandidateGroup>,
    ) -> Result<Vec<DuplicateGroup>> {
        let conn = db.connection();
        let mut all_duplicates = Vec::new();
        let mut group_id = 0i64;

        for candidate in candidates {
            // Get all files in this candidate group
            let mut stmt = conn.prepare(
                "SELECT id, path, name, extension, modified_at, full_hash
                 FROM files
                 WHERE size = ?1 AND partial_hash = ?2",
            )?;

            let files: Vec<FileToHash> = stmt
                .query_map(
                    rusqlite::params![candidate.size, candidate.partial_hash],
                    |row| {
                        Ok(FileToHash {
                            id: row.get(0)?,
                            path: row.get(1)?,
                            name: row.get(2)?,
                            extension: row.get(3)?,
                            modified_at: row.get(4)?,
                            existing_hash: row.get(5)?,
                        })
                    },
                )?
                .filter_map(|r| r.ok())
                .collect();

            if files.len() < 2 {
                continue;
            }

            // Compute full hashes for files that don't have them
            let hashed_files: Vec<(FileToHash, Vec<u8>)> = files
                .into_par_iter()
                .filter_map(|file| {
                    let hash = if let Some(existing) = &file.existing_hash {
                        existing.clone()
                    } else {
                        match compute_full_hash(&file.path) {
                            Ok(h) => h,
                            Err(e) => {
                                warn!("Failed to hash {}: {}", file.path, e);
                                return None;
                            }
                        }
                    };
                    Some((file, hash))
                })
                .collect();

            // Group by full hash
            let mut hash_groups: HashMap<Vec<u8>, Vec<FileToHash>> = HashMap::new();
            for (file, hash) in hashed_files {
                // Update the database with the full hash if it was newly computed
                if file.existing_hash.is_none() {
                    let _ = conn.execute(
                        "UPDATE files SET full_hash = ?1 WHERE id = ?2",
                        rusqlite::params![hash, file.id],
                    );
                }
                hash_groups.entry(hash).or_default().push(file);
            }

            // Create duplicate groups for confirmed duplicates
            for (hash, files) in hash_groups {
                if files.len() > 1 {
                    group_id += 1;
                    let wasted_space = candidate.size * (files.len() as i64 - 1);

                    let mut duplicate_files: Vec<DuplicateFile> = files
                        .into_iter()
                        .map(|f| DuplicateFile {
                            id: f.id,
                            path: f.path,
                            name: f.name,
                            extension: f.extension,
                            modified_at: f.modified_at,
                            is_original: false,
                        })
                        .collect();

                    // Mark the oldest file as original
                    if let Some(oldest_idx) = duplicate_files
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, f)| f.modified_at.unwrap_or(i64::MAX))
                        .map(|(i, _)| i)
                    {
                        duplicate_files[oldest_idx].is_original = true;
                    }

                    all_duplicates.push(DuplicateGroup {
                        id: group_id,
                        hash: hex::encode(&hash),
                        size: candidate.size,
                        files: duplicate_files,
                        wasted_space,
                    });
                }
            }
        }

        // Sort by wasted space descending
        all_duplicates.sort_by(|a, b| b.wasted_space.cmp(&a.wasted_space));

        Ok(all_duplicates)
    }
}

/// Candidate group from phase 1
struct CandidateGroup {
    size: i64,
    partial_hash: Vec<u8>,
}

/// File pending full hash computation
struct FileToHash {
    id: i64,
    path: String,
    name: String,
    extension: Option<String>,
    modified_at: Option<i64>,
    existing_hash: Option<Vec<u8>>,
}

/// Compute full BLAKE3 hash for a file
fn compute_full_hash(path: &str) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut hasher = Hasher::new();
    let mut buffer = vec![0u8; 1024 * 1024]; // 1MB buffer

    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hasher.finalize().as_bytes().to_vec())
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_identical_files_same_hash() {
        let mut file1 = NamedTempFile::new().unwrap();
        let mut file2 = NamedTempFile::new().unwrap();

        let content = b"This is the same content in both files";
        file1.write_all(content).unwrap();
        file2.write_all(content).unwrap();
        file1.flush().unwrap();
        file2.flush().unwrap();

        let hash1 = compute_full_hash(file1.path().to_str().unwrap()).unwrap();
        let hash2 = compute_full_hash(file2.path().to_str().unwrap()).unwrap();

        assert_eq!(hash1, hash2);
    }
}
