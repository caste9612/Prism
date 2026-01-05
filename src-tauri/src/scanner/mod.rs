//! Filesystem scanner module
//!
//! Provides high-performance directory traversal and metadata extraction.

mod hasher;
mod walker;

pub use hasher::compute_partial_hash;
pub use walker::{ScanConfig, Scanner};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// File metadata collected during scanning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub path: String,
    pub name: String,
    pub extension: Option<String>,
    pub size: i64,
    pub created_at: Option<i64>,
    pub modified_at: Option<i64>,
    pub accessed_at: Option<i64>,
    pub attributes: Option<u32>,
    pub partial_hash: Option<Vec<u8>>,
}

/// Per-drive progress information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveProgress {
    pub drive: String,
    pub files_scanned: u64,
    pub total_size: u64,
    pub status: String, // "scanning", "done", "waiting"
    /// Estimated progress percentage (0-100)
    pub progress_percent: f64,
    /// Estimated total size on this drive (from disk info)
    pub estimated_total: u64,
}

/// Progress information during scanning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    pub scan_id: i64,
    pub files_scanned: u64,
    pub total_size: u64,
    pub current_path: String,
    pub files_per_second: f64,
    pub is_complete: bool,
    pub error: Option<String>,
    /// Per-drive progress tracking
    pub drives: HashMap<String, DriveProgress>,
    /// Indexing progress (only set during indexing phase)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexing: Option<IndexingProgress>,
}

/// Progress information during search index building
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexingProgress {
    pub files_indexed: u64,
    pub total_files: u64,
    pub percent: f64,
    pub files_per_second: f64,
}

/// Progress information during incremental scan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncrementalProgress {
    pub phase: String, // "preparing", "scanning", "cleaning", "indexing", "complete"
    pub files_checked: u64,
    pub files_unchanged: u64,
    pub files_updated: u64,
    pub files_new: u64,
    pub files_deleted: u64,
    pub total_files_in_db: u64,
    pub percent: f64,
}

impl Default for IncrementalProgress {
    fn default() -> Self {
        Self {
            phase: "preparing".to_string(),
            files_checked: 0,
            files_unchanged: 0,
            files_updated: 0,
            files_new: 0,
            files_deleted: 0,
            total_files_in_db: 0,
            percent: 0.0,
        }
    }
}

impl Default for ScanProgress {
    fn default() -> Self {
        Self {
            scan_id: 0,
            files_scanned: 0,
            total_size: 0,
            current_path: String::new(),
            files_per_second: 0.0,
            is_complete: false,
            error: None,
            drives: HashMap::new(),
            indexing: None,
        }
    }
}
