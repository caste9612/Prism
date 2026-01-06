//! Duplicate detection and management commands

use crate::duplicates::{DuplicateFinder, DuplicateGroup};
use crate::AppState;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tracing::{debug, error, info, warn};

/// Response for duplicate detection
#[derive(Debug, Serialize)]
pub struct DuplicateResponse {
    pub groups: Vec<DuplicateGroup>,
    pub total_groups: usize,
    pub total_wasted_space: i64,
    pub detection_time_ms: u64,
}

/// Find duplicate files in the database
#[tauri::command]
pub async fn find_duplicates(
    state: State<'_, AppState>,
    min_size: Option<i64>,
) -> Result<DuplicateResponse, String> {
    let start = std::time::Instant::now();
    info!("Starting duplicate detection");

    let db = state.db.lock().await;

    let finder = if let Some(size) = min_size {
        DuplicateFinder::new().with_min_size(size)
    } else {
        DuplicateFinder::new()
    };

    let groups = finder
        .find_duplicates(&db)
        .map_err(|e| format!("Duplicate detection failed: {}", e))?;

    let total_wasted_space: i64 = groups.iter().map(|g| g.wasted_space).sum();
    let detection_time_ms = start.elapsed().as_millis() as u64;

    info!(
        "Duplicate detection complete: {} groups, {} wasted space in {}ms",
        groups.len(),
        total_wasted_space,
        detection_time_ms
    );

    Ok(DuplicateResponse {
        total_groups: groups.len(),
        groups,
        total_wasted_space,
        detection_time_ms,
    })
}

/// Normalize path for comparison (handles case-insensitivity on Windows)
fn normalize_path(path: &str) -> String {
    #[cfg(windows)]
    {
        path.to_uppercase().replace('/', "\\")
    }
    #[cfg(not(windows))]
    {
        path.to_string()
    }
}

/// Delete a duplicate file with path validation
#[tauri::command]
pub async fn delete_duplicate(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    file_id: i64,
    file_path: String,
) -> Result<(), String> {
    info!("Deleting duplicate file: {} (id: {})", file_path, file_id);

    // Validate path exists and matches
    let path = std::path::Path::new(&file_path);

    // Check for symlinks first
    if path.is_symlink() {
        return Err("Cannot delete symlinks for safety reasons".to_string());
    }

    if !path.exists() {
        return Err("File does not exist".to_string());
    }
    if !path.is_file() {
        return Err("Path is not a file".to_string());
    }

    // Delete from filesystem
    std::fs::remove_file(&file_path).map_err(|e| format!("Failed to delete file: {}", e))?;

    // Delete from database with path validation
    let db = state.db.lock().await;
    db.delete_file(file_id, &file_path)
        .map_err(|e| format!("Failed to remove from database: {}", e))?;

    // Emit stats update
    let _ = app_handle.emit("stats-updated", ());

    info!("Successfully deleted: {}", file_path);
    Ok(())
}

/// Response for batch delete operation
#[derive(Debug, Serialize)]
pub struct BatchDeleteResponse {
    pub deleted: usize,
    pub errors: Vec<String>,
}

/// Delete multiple duplicate files with path validation
#[tauri::command]
pub async fn delete_duplicates_batch(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    files: Vec<(i64, String)>,
) -> Result<BatchDeleteResponse, String> {
    info!("Batch deleting {} duplicate files", files.len());

    let mut deleted = 0;
    let mut errors = Vec::new();
    let mut db_files = Vec::new();

    // First, delete from filesystem
    for (file_id, file_path) in &files {
        let path = std::path::Path::new(file_path);

        // Check for symlinks - don't delete symlinks pointing elsewhere
        if path.is_symlink() {
            errors.push(format!("{}: is a symlink, skipping", file_path));
            continue;
        }

        if !path.exists() || !path.is_file() {
            errors.push(format!("{}: file not found", file_path));
            continue;
        }

        if let Err(e) = std::fs::remove_file(file_path) {
            errors.push(format!("{}: {}", file_path, e));
            continue;
        }

        db_files.push((*file_id, file_path.clone()));
        deleted += 1;
    }

    // Then, delete from database in a transaction
    if !db_files.is_empty() {
        let db = state.db.lock().await;
        if let Err(e) = db.delete_files_batch(&db_files) {
            // Propagate database error instead of silently logging
            let db_error = format!("Database sync failed after deleting {} files: {}", deleted, e);
            error!("{}", db_error);
            errors.push(db_error);
        }
    }

    if !errors.is_empty() {
        warn!("Some files failed to delete: {:?}", errors);
    }

    // Emit stats update
    let _ = app_handle.emit("stats-updated", ());

    info!(
        "Batch delete complete: {} of {} files deleted",
        deleted,
        files.len()
    );

    Ok(BatchDeleteResponse { deleted, errors })
}

/// Similar image group
#[derive(Debug, Serialize)]
pub struct SimilarImageGroup {
    pub id: i64,
    pub images: Vec<SimilarImage>,
    pub similarity_threshold: u32,
}

/// A similar image with its perceptual hash
#[derive(Debug, Serialize)]
pub struct SimilarImage {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub size: i64,
    pub phash: String,
    pub distance_from_first: u32,
}

/// Response for similar image detection
#[derive(Debug, Serialize)]
pub struct SimilarImagesResponse {
    pub groups: Vec<SimilarImageGroup>,
    pub total_groups: usize,
    pub total_images: usize,
    pub detection_time_ms: u64,
}

/// Find similar images using perceptual hashing
#[tauri::command]
pub async fn find_similar_images(
    state: State<'_, AppState>,
    threshold: Option<u32>,
) -> Result<SimilarImagesResponse, String> {
    use crate::duplicates::{compute_dhash, hamming_distance, is_image_file};
    use std::collections::HashMap;
    use std::path::Path;

    let start = std::time::Instant::now();
    let similarity_threshold = threshold.unwrap_or(10);

    info!(
        "Starting similar image detection with threshold {}",
        similarity_threshold
    );

    let db = state.db.lock().await;
    let conn = db.connection();

    // Get all image files from database
    let mut stmt = conn
        .prepare(
            "SELECT id, path, name, size, perceptual_hash FROM files
         WHERE extension IN ('jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp')
         AND size > 1024
         ORDER BY size DESC",
        )
        .map_err(|e| e.to_string())?;

    let image_files: Vec<(i64, String, String, i64, Option<Vec<u8>>)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    info!("Found {} image files to analyze", image_files.len());

    // Compute perceptual hashes for images that don't have them
    let mut hashes: HashMap<i64, (String, String, i64, u64)> = HashMap::new();

    for (id, path, name, size, existing_hash) in image_files {
        let phash = if let Some(hash_bytes) = existing_hash {
            if hash_bytes.len() >= 8 {
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&hash_bytes[..8]);
                u64::from_le_bytes(bytes)
            } else {
                continue;
            }
        } else {
            let path_obj = Path::new(&path);
            if !is_image_file(path_obj) || !path_obj.exists() {
                continue;
            }

            match compute_dhash(path_obj) {
                Ok(hash) => {
                    let hash_bytes = hash.to_le_bytes().to_vec();
                    let _ = conn.execute(
                        "UPDATE files SET perceptual_hash = ?1 WHERE id = ?2",
                        rusqlite::params![hash_bytes, id],
                    );
                    hash
                }
                Err(e) => {
                    debug!("Failed to compute phash for {}: {}", path, e);
                    continue;
                }
            }
        };

        hashes.insert(id, (path, name, size, phash));
    }

    info!("Computed {} perceptual hashes", hashes.len());

    // Find similar image groups using optimized algorithm
    let mut processed: std::collections::HashSet<i64> = std::collections::HashSet::new();
    let mut groups: Vec<SimilarImageGroup> = Vec::new();
    let mut group_id = 0i64;

    let hash_vec: Vec<(i64, String, String, i64, u64)> = hashes
        .into_iter()
        .map(|(id, (path, name, size, hash))| (id, path, name, size, hash))
        .collect();

    for i in 0..hash_vec.len() {
        let (id1, path1, name1, size1, hash1) = &hash_vec[i];

        if processed.contains(id1) {
            continue;
        }

        let mut group_images = vec![SimilarImage {
            id: *id1,
            path: path1.clone(),
            name: name1.clone(),
            size: *size1,
            phash: format!("{:016x}", hash1),
            distance_from_first: 0,
        }];

        for j in (i + 1)..hash_vec.len() {
            let (id2, path2, name2, size2, hash2) = &hash_vec[j];

            if processed.contains(id2) {
                continue;
            }

            let distance = hamming_distance(*hash1, *hash2);
            if distance <= similarity_threshold {
                group_images.push(SimilarImage {
                    id: *id2,
                    path: path2.clone(),
                    name: name2.clone(),
                    size: *size2,
                    phash: format!("{:016x}", hash2),
                    distance_from_first: distance,
                });
                processed.insert(*id2);
            }
        }

        if group_images.len() > 1 {
            processed.insert(*id1);
            group_id += 1;
            groups.push(SimilarImageGroup {
                id: group_id,
                images: group_images,
                similarity_threshold,
            });
        }
    }

    let detection_time_ms = start.elapsed().as_millis() as u64;
    let total_images: usize = groups.iter().map(|g| g.images.len()).sum();

    info!(
        "Similar image detection complete: {} groups, {} images in {}ms",
        groups.len(),
        total_images,
        detection_time_ms
    );

    Ok(SimilarImagesResponse {
        total_groups: groups.len(),
        groups,
        total_images,
        detection_time_ms,
    })
}
