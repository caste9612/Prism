//! Perceptual hashing for image similarity detection
//!
//! Implements Average Hash (aHash) for fast image comparison.
//! Similar images will have similar hashes even with resizing,
//! compression, or minor edits.

use anyhow::Result;
use image::imageops::FilterType;
use std::path::Path;

/// Size of the hash grid (8x8 = 64 bits)
const HASH_SIZE: u32 = 8;

/// Compute perceptual hash (aHash) for an image
pub fn compute_phash(path: &Path) -> Result<u64> {
    // Load and decode image
    let img = image::open(path)?;

    // Convert to grayscale and resize to 8x8
    let small = img
        .grayscale()
        .resize_exact(HASH_SIZE, HASH_SIZE, FilterType::Lanczos3);

    // Calculate average pixel value
    let pixels: Vec<u8> = small.to_luma8().into_raw();
    let avg: u64 = pixels.iter().map(|&p| p as u64).sum::<u64>() / (HASH_SIZE * HASH_SIZE) as u64;

    // Generate hash: 1 if pixel > average, 0 otherwise
    let mut hash: u64 = 0;
    for (i, &pixel) in pixels.iter().enumerate() {
        if pixel as u64 > avg {
            hash |= 1 << i;
        }
    }

    Ok(hash)
}

/// Compute difference hash (dHash) - more robust than aHash
pub fn compute_dhash(path: &Path) -> Result<u64> {
    // Load and decode image
    let img = image::open(path)?;

    // Convert to grayscale and resize to 9x8 (need 9 columns for 8 differences)
    let small = img
        .grayscale()
        .resize_exact(HASH_SIZE + 1, HASH_SIZE, FilterType::Lanczos3);

    let gray = small.to_luma8();
    let mut hash: u64 = 0;
    let mut bit = 0;

    // Compare adjacent pixels horizontally
    for y in 0..HASH_SIZE {
        for x in 0..HASH_SIZE {
            let left = gray.get_pixel(x, y)[0];
            let right = gray.get_pixel(x + 1, y)[0];

            if left > right {
                hash |= 1 << bit;
            }
            bit += 1;
        }
    }

    Ok(hash)
}

/// Calculate Hamming distance between two hashes
/// Lower distance = more similar images
pub fn hamming_distance(hash1: u64, hash2: u64) -> u32 {
    (hash1 ^ hash2).count_ones()
}

/// Check if two images are similar based on their hashes
/// threshold: maximum Hamming distance to consider similar (typically 5-10)
pub fn are_similar(hash1: u64, hash2: u64, threshold: u32) -> bool {
    hamming_distance(hash1, hash2) <= threshold
}

/// Check if a file is an image based on extension
pub fn is_image_file(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => matches!(
            ext.to_lowercase().as_str(),
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp"
        ),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hamming_distance() {
        assert_eq!(hamming_distance(0b1111, 0b1111), 0);
        assert_eq!(hamming_distance(0b1111, 0b1110), 1);
        assert_eq!(hamming_distance(0b1111, 0b0000), 4);
    }

    #[test]
    fn test_are_similar() {
        assert!(are_similar(0b1111, 0b1111, 5));
        assert!(are_similar(0b1111, 0b1110, 5));
        assert!(!are_similar(0b11111111, 0b00000000, 5));
    }
}
