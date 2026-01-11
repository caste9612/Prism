//! Duplicate file finder with two-phase hashing

use crate::database::Database;
use anyhow::Result;
use blake3::Hasher;
use rayon::prelude::*;
use serde::Serialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Minimum file size to consider for deduplication (skip tiny files)
const MIN_DUPLICATE_SIZE: i64 = 1024; // 1KB

/// Progress information during duplicate detection
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateProgress {
    pub phase: String,
    pub percent: f64,
    pub candidate_groups: usize,
    pub confirmed_groups: usize,
    pub files_processed: usize,
    pub total_files: usize,
    pub message: String,
}

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

    /// Find all duplicate files in the database (without progress callback)
    pub fn find_duplicates(&self, db: &Database) -> Result<Vec<DuplicateGroup>> {
        self.find_duplicates_with_progress(db, |_| {})
    }

    /// Find all duplicate files in the database with progress callback
    pub fn find_duplicates_with_progress<F>(
        &self,
        db: &Database,
        mut on_progress: F,
    ) -> Result<Vec<DuplicateGroup>>
    where
        F: FnMut(DuplicateProgress),
    {
        info!("=== DUPLICATE DETECTION START ===");
        info!("Minimum file size: {} bytes", self.min_size);

        // Emit initial progress
        on_progress(DuplicateProgress {
            phase: "candidates".to_string(),
            percent: 0.0,
            candidate_groups: 0,
            confirmed_groups: 0,
            files_processed: 0,
            total_files: 0,
            message: "Finding candidate duplicates by size and partial hash...".to_string(),
        });

        // Phase 1: Find candidates by size and partial hash
        let candidates = self.find_candidates(db)?;
        let candidate_count = candidates.len();
        info!(
            "Phase 1 complete: Found {} candidate groups",
            candidate_count
        );

        if candidates.is_empty() {
            info!("No duplicate candidates found, exiting early");
            on_progress(DuplicateProgress {
                phase: "complete".to_string(),
                percent: 100.0,
                candidate_groups: 0,
                confirmed_groups: 0,
                files_processed: 0,
                total_files: 0,
                message: "No duplicate candidates found".to_string(),
            });
            return Ok(Vec::new());
        }

        // Count total files in candidates for progress
        let total_candidate_files: usize = candidates.iter().map(|c| c.file_count).sum();
        info!(
            "Total files to analyze: {} across {} groups",
            total_candidate_files, candidate_count
        );

        on_progress(DuplicateProgress {
            phase: "hashing".to_string(),
            percent: 10.0,
            candidate_groups: candidate_count,
            confirmed_groups: 0,
            files_processed: 0,
            total_files: total_candidate_files,
            message: format!(
                "Computing full hashes for {} files in {} groups...",
                total_candidate_files, candidate_count
            ),
        });

        // Phase 2: Compute full hashes for candidates and confirm duplicates
        let duplicates =
            self.confirm_duplicates_with_progress(db, candidates, total_candidate_files, |progress| {
                on_progress(progress);
            })?;

        let total_wasted: i64 = duplicates.iter().map(|g| g.wasted_space).sum();
        let total_duplicate_files: usize = duplicates.iter().map(|g| g.files.len()).sum();

        info!("=== DUPLICATE DETECTION COMPLETE ===");
        info!("Confirmed {} duplicate groups", duplicates.len());
        info!("Total duplicate files: {}", total_duplicate_files);
        info!(
            "Total wasted space: {} bytes ({:.2} MB)",
            total_wasted,
            total_wasted as f64 / 1_048_576.0
        );

        on_progress(DuplicateProgress {
            phase: "complete".to_string(),
            percent: 100.0,
            candidate_groups: candidate_count,
            confirmed_groups: duplicates.len(),
            files_processed: total_candidate_files,
            total_files: total_candidate_files,
            message: format!(
                "Found {} duplicate groups ({} files)",
                duplicates.len(),
                total_duplicate_files
            ),
        });

        Ok(duplicates)
    }

    /// Phase 1: Find files with same size, computing partial hashes on-demand
    /// Optimized version using single query and batch updates
    fn find_candidates(&self, db: &Database) -> Result<Vec<CandidateGroup>> {
        use crate::scanner::compute_partial_hash;
        use std::path::Path;
        use std::sync::Mutex;

        let conn = db.connection();

        info!("Phase 1: Finding files with duplicate sizes...");

        // Single optimized query: get ALL files that have duplicate sizes at once
        // This uses a subquery to find sizes with duplicates, then joins back
        let mut stmt = conn.prepare(
            "SELECT f.id, f.path, f.size, f.partial_hash
             FROM files f
             INNER JOIN (
                 SELECT size
                 FROM files
                 WHERE size >= ?1
                 GROUP BY size
                 HAVING COUNT(*) > 1
             ) dup ON f.size = dup.size
             ORDER BY f.size DESC",
        )?;

        // Collect all files with duplicate sizes
        let files: Vec<(i64, String, i64, Option<Vec<u8>>)> = stmt
            .query_map([self.min_size], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        let total_files = files.len();
        info!("Found {} files with duplicate sizes", total_files);

        if files.is_empty() {
            return Ok(Vec::new());
        }

        // Separate files that need hash computation
        let files_with_hash: Vec<_> = files
            .iter()
            .filter(|(_, _, _, h)| h.is_some())
            .cloned()
            .collect();
        let files_needing_hash: Vec<_> = files
            .iter()
            .filter(|(_, _, _, h)| h.is_none())
            .cloned()
            .collect();

        info!(
            "{} files have partial hash, {} need computation",
            files_with_hash.len(),
            files_needing_hash.len()
        );

        // Compute hashes in parallel for files that need it
        let hash_updates: Arc<Mutex<Vec<(i64, Vec<u8>)>>> = Arc::new(Mutex::new(Vec::new()));
        let files_hashed = Arc::new(AtomicUsize::new(0));

        if !files_needing_hash.is_empty() {
            info!("Computing partial hashes for {} files in parallel...", files_needing_hash.len());

            let batch_size = 1000;
            let total_batches = (files_needing_hash.len() + batch_size - 1) / batch_size;

            for (batch_idx, chunk) in files_needing_hash.chunks(batch_size).enumerate() {
                if batch_idx % 10 == 0 {
                    info!("Processing batch {}/{}", batch_idx + 1, total_batches);
                }

                let results: Vec<_> = chunk
                    .par_iter()
                    .filter_map(|(id, path, _size, _)| {
                        let path_obj = Path::new(path);
                        if !path_obj.exists() {
                            return None;
                        }
                        match compute_partial_hash(path_obj) {
                            Ok(h) => Some((*id, h)),
                            Err(e) => {
                                debug!("Failed to hash {}: {}", path, e);
                                None
                            }
                        }
                    })
                    .collect();

                files_hashed.fetch_add(results.len(), Ordering::Relaxed);
                hash_updates.lock().unwrap().extend(results);
            }
        }

        let hash_updates = Arc::try_unwrap(hash_updates).unwrap().into_inner().unwrap();
        let files_hashed_count = files_hashed.load(Ordering::Relaxed);

        if files_hashed_count > 0 {
            info!("Computed {} partial hashes, saving to database...", files_hashed_count);

            // Batch update hashes in database
            let tx = conn.unchecked_transaction()?;
            {
                let mut update_stmt = tx.prepare_cached(
                    "UPDATE files SET partial_hash = ?1 WHERE id = ?2"
                )?;

                for (id, hash) in &hash_updates {
                    let _ = update_stmt.execute(rusqlite::params![hash, id]);
                }
            }
            tx.commit()?;
            info!("Saved {} hashes to database", hash_updates.len());
        }

        // Build a map of id -> hash for newly computed hashes
        let new_hashes: HashMap<i64, Vec<u8>> = hash_updates.into_iter().collect();

        // Group files by (size, partial_hash)
        let mut candidates: HashMap<(i64, Vec<u8>), usize> = HashMap::new();

        // Process files that already had hashes
        for (_, _, size, hash_opt) in &files_with_hash {
            if let Some(hash) = hash_opt {
                *candidates.entry((*size, hash.clone())).or_insert(0) += 1;
            }
        }

        // Process files that got new hashes
        for (id, _, size, _) in &files_needing_hash {
            if let Some(hash) = new_hashes.get(id) {
                *candidates.entry((*size, hash.clone())).or_insert(0) += 1;
            }
        }

        // Filter to only groups with 2+ files
        let result: Vec<CandidateGroup> = candidates
            .into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|((size, partial_hash), count)| CandidateGroup {
                size,
                partial_hash,
                file_count: count,
            })
            .collect();

        // Sort by size descending
        let mut result = result;
        result.sort_by(|a, b| b.size.cmp(&a.size));

        info!(
            "Found {} candidate groups by size+partial_hash",
            result.len()
        );

        // Log some examples
        for (i, c) in result.iter().take(5).enumerate() {
            debug!(
                "  Candidate {}: {} files, {} bytes each",
                i + 1,
                c.file_count,
                c.size
            );
        }

        Ok(result)
    }

    /// Phase 2: Compute full hashes and confirm true duplicates with progress
    fn confirm_duplicates_with_progress<F>(
        &self,
        db: &Database,
        candidates: Vec<CandidateGroup>,
        total_files: usize,
        mut on_progress: F,
    ) -> Result<Vec<DuplicateGroup>>
    where
        F: FnMut(DuplicateProgress),
    {
        let conn = db.connection();
        let mut all_duplicates = Vec::new();
        let mut group_id = 0i64;

        let files_processed = Arc::new(AtomicUsize::new(0));
        let total_candidates = candidates.len();
        let mut last_progress_percent = 0;

        info!("Phase 2: Computing full hashes for {} candidate groups...", total_candidates);

        for (idx, candidate) in candidates.into_iter().enumerate() {
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

            let files_in_group = files.len();
            debug!(
                "Processing candidate group {}/{}: {} files, {} bytes each",
                idx + 1,
                total_candidates,
                files_in_group,
                candidate.size
            );

            // Count how many need hashing vs already have hash
            let needs_hashing = files.iter().filter(|f| f.existing_hash.is_none()).count();
            let has_cached = files_in_group - needs_hashing;

            if needs_hashing > 0 {
                debug!("  {} files need hashing, {} have cached hash", needs_hashing, has_cached);
            }

            // Compute full hashes for files that don't have them
            let files_processed_clone = Arc::clone(&files_processed);
            let hashed_files: Vec<(FileToHash, Vec<u8>)> = files
                .into_par_iter()
                .filter_map(|file| {
                    let hash = if let Some(existing) = &file.existing_hash {
                        existing.clone()
                    } else {
                        match compute_full_hash(&file.path) {
                            Ok(h) => {
                                debug!("  Hashed: {}", file.path);
                                h
                            }
                            Err(e) => {
                                warn!("  Failed to hash {}: {}", file.path, e);
                                return None;
                            }
                        }
                    };
                    files_processed_clone.fetch_add(1, Ordering::Relaxed);
                    Some((file, hash))
                })
                .collect();

            // Emit progress every few groups or when significant progress made
            let current_processed = files_processed.load(Ordering::Relaxed);
            let progress_percent = if total_files > 0 {
                10 + ((current_processed as f64 / total_files as f64) * 85.0) as i32
            } else {
                10 + ((idx as f64 / total_candidates as f64) * 85.0) as i32
            };

            if progress_percent > last_progress_percent + 5 || idx == total_candidates - 1 {
                last_progress_percent = progress_percent;
                on_progress(DuplicateProgress {
                    phase: "hashing".to_string(),
                    percent: progress_percent as f64,
                    candidate_groups: total_candidates,
                    confirmed_groups: all_duplicates.len(),
                    files_processed: current_processed,
                    total_files,
                    message: format!(
                        "Processing group {}/{} ({} duplicates found)...",
                        idx + 1,
                        total_candidates,
                        all_duplicates.len()
                    ),
                });
            }

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

                    debug!(
                        "  Confirmed duplicate group: {} files, wasted {} bytes",
                        files.len(),
                        wasted_space
                    );

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
                        debug!("    Original: {}", duplicate_files[oldest_idx].path);
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

        // Final grouping progress
        on_progress(DuplicateProgress {
            phase: "grouping".to_string(),
            percent: 98.0,
            candidate_groups: total_candidates,
            confirmed_groups: all_duplicates.len(),
            files_processed: files_processed.load(Ordering::Relaxed),
            total_files,
            message: "Sorting and finalizing results...".to_string(),
        });

        // Sort by wasted space descending
        all_duplicates.sort_by(|a, b| b.wasted_space.cmp(&a.wasted_space));

        // Log top 10 largest duplicate groups
        info!("Top 10 largest duplicate groups:");
        for (i, group) in all_duplicates.iter().take(10).enumerate() {
            info!(
                "  {}. {} files, {} bytes each, {} bytes wasted",
                i + 1,
                group.files.len(),
                group.size,
                group.wasted_space
            );
            for file in group.files.iter().take(3) {
                info!("      - {}", file.path);
            }
            if group.files.len() > 3 {
                info!("      ... and {} more", group.files.len() - 3);
            }
        }

        Ok(all_duplicates)
    }
}

/// Candidate group from phase 1
struct CandidateGroup {
    size: i64,
    partial_hash: Vec<u8>,
    file_count: usize,
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
