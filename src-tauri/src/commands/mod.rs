//! Tauri commands for frontend-backend communication
//!
//! This module is organized into submodules for better maintainability:
//! - `stats` - Application statistics
//! - `scan` - Scanning functionality
//! - `search` - Search functionality
//! - `duplicates` - Duplicate detection and management
//! - `analytics` - Analytics and distribution data
//! - `drives` - Drive detection and management
//! - `export` - Export functionality
//! - `tree` - Tree view commands
//! - `utils` - Utility commands

pub mod analytics;
pub mod drives;
pub mod duplicates;
pub mod export;
pub mod scan;
pub mod search;
pub mod stats;
pub mod tree;
pub mod utils;
pub mod verify;

// Re-export all commands and types for backward compatibility
pub use analytics::{
    get_extension_distribution, get_file_type_distribution, get_folder_contents,
    get_folder_sizes, get_size_distribution, ExtensionCategory, ExtensionDetail,
    FileTypeCategory, FolderItem, FolderSize, SizeCategory,
};

pub use drives::{
    get_available_drives, get_drive_stats, get_drives_status, remove_offline_drive,
    DriveInfo, DriveOnlineStatus, DriveStatus,
};

pub use duplicates::{
    delete_duplicate, delete_duplicates_batch, find_duplicates, find_similar_images,
    DuplicateResponse, SimilarImage, SimilarImageGroup, SimilarImagesResponse,
};

pub use export::{export_to_csv, export_to_json, ExportFile};

pub use scan::{auto_scan_drives, start_incremental_scan, start_scan, start_smart_scan, ScanRequest};

pub use search::{
    quick_search, search_files, search_files_advanced, AdvancedSearchRequest,
    AdvancedSearchResponse, QuickSearchResponse, QuickSearchResult, SearchRequest,
};

pub use stats::{get_recent_scans, get_stats, AppStats};

pub use tree::{get_directory_tree, get_tree_children, NodeType, TreeNode};

pub use utils::{clear_database, open_in_explorer};

pub use verify::{
    quick_backup_check, verify_cross_disk, FileVerificationResult, VerificationSummary,
};
