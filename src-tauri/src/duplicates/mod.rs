//! Duplicate detection module for Prism
//!
//! Implements two-phase hashing:
//! 1. Partial hash (first 64KB) for quick candidate identification
//! 2. Full BLAKE3 hash for exact duplicate confirmation
//! 3. Perceptual hash for similar image detection

mod finder;
pub mod phash;

pub use finder::{DuplicateFinder, DuplicateGroup, DuplicateFile, DuplicateProgress};
pub use phash::{compute_phash, compute_dhash, hamming_distance, is_image_file};
