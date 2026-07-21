// Hash Module - Hashing and deduplication for PixPipe
// This module contains all hash computation and duplicate detection logic.

#![allow(dead_code)]

use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Compute hash for a file
pub fn compute_hash(path: &Path, algorithm: &str) -> Result<String> {
    use std::fs::File;
    use std::io::Read;

    let mut file = File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    match algorithm {
        "sha256" => {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&buffer);
            Ok(format!("{:x}", hasher.finalize()))
        }
        "sha512" => {
            use sha2::{Digest, Sha512};
            let mut hasher = Sha512::new();
            hasher.update(&buffer);
            Ok(format!("{:x}", hasher.finalize()))
        }
        "blake3" => {
            let hash = blake3::hash(&buffer);
            Ok(hash.to_hex().to_string())
        }
        "xxh3" => {
            use xxhash_rust::xxh3::xxh3_64;
            let hash = xxh3_64(&buffer);
            Ok(format!("{hash:016x}"))
        }
        _ => {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&buffer);
            Ok(format!("{:x}", hasher.finalize()))
        }
    }
}

/// Compute perceptual hash for image similarity detection
pub fn compute_phash(path: &Path) -> Result<u64> {
    // Simple perceptual hash using image resize + average
    // For real implementation, would use img_hash or similar crate
    use std::fs::File;
    use std::io::Read;

    let mut file = File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    // Simple hash based on file content (placeholder)
    // Real implementation would:
    // 1. Decode image
    // 2. Resize to 8x8
    // 3. Convert to grayscale
    // 4. Compute DCT
    // 5. Generate 64-bit hash
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&buffer);
    let result = hasher.finalize();

    let mut hash = 0u64;
    for i in 0..8 {
        hash = (hash << 8) | u64::from(result[i]);
    }

    Ok(hash)
}

/// Compute Hamming distance between two perceptual hashes
pub fn phash_distance(hash1: u64, hash2: u64) -> u32 {
    (hash1 ^ hash2).count_ones()
}

/// Find duplicate files by hash
pub fn find_duplicates(files: &[(String, String)]) -> HashMap<String, Vec<String>> {
    let mut hash_map: HashMap<String, Vec<String>> = HashMap::new();

    for (path, hash) in files {
        hash_map.entry(hash.clone()).or_default().push(path.clone());
    }

    // Filter to only groups with more than one file
    hash_map
        .into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .collect()
}

/// Find similar images by perceptual hash distance
pub fn find_similar_images(files: &[(String, u64)], threshold: u32) -> Vec<(String, String, u32)> {
    let mut similar = Vec::new();

    for i in 0..files.len() {
        for j in (i + 1)..files.len() {
            let distance = phash_distance(files[i].1, files[j].1);
            if distance <= threshold {
                similar.push((files[i].0.clone(), files[j].0.clone(), distance));
            }
        }
    }

    similar.sort_by_key(|&(_, _, d)| d);
    similar
}

/// Compute hash for directory (hash of all file hashes)
pub fn compute_dir_hash(dir: &Path, algorithm: &str) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::fs;

    let mut hasher = Sha256::new();
    let mut entries: Vec<_> = fs::read_dir(dir)?.filter_map(std::result::Result::ok).collect();

    // Sort for deterministic hash
    entries.sort_by_key(std::fs::DirEntry::path);

    for entry in entries {
        let path = entry.path();
        if path.is_file() {
            let file_hash = compute_hash(&path, algorithm)?;
            hasher.update(file_hash.as_bytes());
        } else if path.is_dir() {
            let dir_hash = compute_dir_hash(&path, algorithm)?;
            hasher.update(dir_hash.as_bytes());
        }
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Verify file integrity against expected hash
pub fn verify_hash(path: &Path, expected: &str, algorithm: &str) -> Result<bool> {
    let actual = compute_hash(path, algorithm)?;
    Ok(actual == expected)
}

/// Batch hash computation with progress callback
pub fn batch_hash(
    files: &[PathBuf],
    algorithm: &str,
    mut on_progress: impl FnMut(usize, usize),
) -> Result<Vec<(String, String)>> {
    let mut results = Vec::new();
    let total = files.len();

    for (i, path) in files.iter().enumerate() {
        let hash = compute_hash(path, algorithm)?;
        results.push((path.to_string_lossy().to_string(), hash));
        on_progress(i + 1, total);
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    #[test]
    fn test_compute_hash_sha256() {
        let temp = Path::new("test_hash.txt");
        fs::write(temp, "hello world").unwrap();

        let hash = compute_hash(temp, "sha256").unwrap();
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // SHA256 hex is 64 chars

        fs::remove_file(temp).unwrap();
    }

    #[test]
    fn test_phash_distance() {
        let hash1 = 0b1010_1010_1010_1010;
        let hash2 = 0b1010_1010_1010_1011;
        assert_eq!(phash_distance(hash1, hash2), 1);

        let hash3 = 0b0101_0101_0101_0101;
        assert_eq!(phash_distance(hash1, hash3), 16);
    }

    #[test]
    fn test_find_duplicates() {
        let files = vec![
            ("a.jpg".to_string(), "hash1".to_string()),
            ("b.jpg".to_string(), "hash1".to_string()),
            ("c.jpg".to_string(), "hash2".to_string()),
        ];

        let dupes = find_duplicates(&files);
        assert_eq!(dupes.len(), 1);
        assert_eq!(dupes["hash1"].len(), 2);
    }
}
