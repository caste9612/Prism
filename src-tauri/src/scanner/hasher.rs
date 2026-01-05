//! File hashing utilities using BLAKE3

use anyhow::Result;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// Size of partial hash (first N bytes of file)
const PARTIAL_HASH_SIZE: usize = 64 * 1024; // 64KB

/// Compute a partial hash of the first 64KB of a file
pub fn compute_partial_hash(path: &Path) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut buffer = vec![0u8; PARTIAL_HASH_SIZE];

    let bytes_read = reader.read(&mut buffer)?;
    buffer.truncate(bytes_read);

    let hash = blake3::hash(&buffer);
    Ok(hash.as_bytes().to_vec())
}

/// Compute the full hash of a file (used in Phase 3 for duplicate verification)
#[allow(dead_code)]
pub fn compute_full_hash(path: &Path) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = blake3::Hasher::new();

    let mut buffer = vec![0u8; 1024 * 1024]; // 1MB buffer
    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let hash = hasher.finalize();
    Ok(hash.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_partial_hash() {
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"Hello, World!").unwrap();

        let hash = compute_partial_hash(temp_file.path()).unwrap();
        assert_eq!(hash.len(), 32); // BLAKE3 produces 32-byte hashes
    }

    #[test]
    fn test_full_hash() {
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"Hello, World!").unwrap();

        let hash = compute_full_hash(temp_file.path()).unwrap();
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_same_content_same_hash() {
        let mut temp1 = NamedTempFile::new().unwrap();
        let mut temp2 = NamedTempFile::new().unwrap();

        temp1.write_all(b"Same content").unwrap();
        temp2.write_all(b"Same content").unwrap();

        let hash1 = compute_full_hash(temp1.path()).unwrap();
        let hash2 = compute_full_hash(temp2.path()).unwrap();

        assert_eq!(hash1, hash2);
    }
}
