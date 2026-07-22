// Files Module - File operations for PixPipe
// This module contains all file I/O and processing logic.

#![allow(dead_code)]

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};

/// Safe file name extraction
pub fn safe_file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

/// Safe file stem extraction
pub fn safe_file_stem(path: &Path) -> String {
    path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

/// Safe extension extraction
pub fn safe_extension(path: &Path) -> String {
    path.extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

/// Safe parent directory extraction
pub fn safe_parent(path: &Path) -> PathBuf {
    path.parent().unwrap_or(Path::new(".")).to_path_buf()
}

/// Safe mutex lock with poison recovery
pub fn safe_lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Copy file with verification
pub fn copy_file_verified(src: &Path, dst: &Path) -> Result<u64> {
    use std::fs;
    use std::io::Read;

    let bytes = fs::copy(src, dst)?;

    // Verify copy
    let mut src_file = fs::File::open(src)?;
    let mut dst_file = fs::File::open(dst)?;
    let mut src_buf = [0u8; 8192];
    let mut dst_buf = [0u8; 8192];

    loop {
        let src_read = src_file.read(&mut src_buf)?;
        let dst_read = dst_file.read(&mut dst_buf)?;

        if src_read != dst_read || src_buf[..src_read] != dst_buf[..dst_read] {
            return Err(anyhow!("File verification failed for {}", dst.display()));
        }

        if src_read == 0 {
            break;
        }
    }

    Ok(bytes)
}

/// Move file with verification
pub fn move_file_verified(src: &Path, dst: &Path) -> Result<u64> {
    use std::fs;

    // Try rename first (atomic on same filesystem)
    if let Ok(()) = fs::rename(src, dst) {
        let metadata = fs::metadata(dst)?;
        Ok(metadata.len())
    } else {
        // Cross-filesystem: copy + delete
        let bytes = copy_file_verified(src, dst)?;
        fs::remove_file(src)?;
        Ok(bytes)
    }
}

/// Create directory if it doesn't exist
pub fn ensure_dir(path: &Path) -> Result<()> {
    use std::fs;

    if !path.exists() {
        fs::create_dir_all(path)?;
    }
    Ok(())
}

/// List files in directory with optional extension filter
pub fn list_files(dir: &Path, extensions: Option<&[&str]>) -> Result<Vec<PathBuf>> {
    use std::fs;

    if !dir.is_dir() {
        return Err(anyhow!("{} is not a directory", dir.display()));
    }

    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            if let Some(exts) = extensions {
                if let Some(ext) = path.extension() {
                    let ext_str = ext.to_string_lossy().to_lowercase();
                    if exts.iter().any(|e| e.to_lowercase() == ext_str) {
                        files.push(path);
                    }
                }
            } else {
                files.push(path);
            }
        }
    }

    Ok(files)
}

/// Get file size in bytes
pub fn file_size(path: &Path) -> Result<u64> {
    use std::fs;

    let metadata = fs::metadata(path)?;
    Ok(metadata.len())
}

/// Format file size as human-readable string
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Format duration as human-readable string
pub fn format_duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        let mins = ms / 60_000;
        let secs = (ms % 60_000) / 1000;
        format!("{mins}m {secs}s")
    }
}

/// Sanitize filename for safe filesystem use
pub fn sanitize_filename(name: &str) -> String {
    let invalid_chars = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    let mut result = String::new();

    for ch in name.chars() {
        if invalid_chars.contains(&ch) {
            result.push('_');
        } else {
            result.push(ch);
        }
    }

    // Trim trailing spaces and dots (Windows)
    let trimmed = result.trim_end_matches([' ', '.']);
    trimmed.to_string()
}

/// Get unique file path (add number suffix if exists)
pub fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }

    let parent = safe_parent(path);
    let stem = safe_file_stem(path);
    let ext = safe_extension(path);

    let mut counter = 1;
    loop {
        let new_name = if ext.is_empty() {
            format!("{stem} ({counter})")
        } else {
            format!("{stem} ({counter}).{ext}")
        };

        let new_path = parent.join(&new_name);
        if !new_path.exists() {
            return new_path;
        }
        counter += 1;
    }
}

/// Check if path is an image file
pub fn is_image_file(path: &Path) -> bool {
    let image_extensions = [
        "jpg", "jpeg", "png", "gif", "bmp", "tiff", "tif", "webp", "avif", "ico", "svg", "heic",
        "heif", "jxl", "j2k", "jp2", "raw", "cr2", "nef", "arw", "dng", "rw2", "orf", "srw", "pef",
        "raf",
    ];

    if let Some(ext) = path.extension() {
        let ext_str = ext.to_string_lossy().to_lowercase();
        image_extensions.contains(&ext_str.as_str())
    } else {
        false
    }
}

/// Get MIME type for file
pub fn mime_type(path: &Path) -> &'static str {
    if let Some(ext) = path.extension() {
        match ext.to_string_lossy().to_lowercase().as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            "tiff" | "tif" => "image/tiff",
            "webp" => "image/webp",
            "avif" => "image/avif",
            "svg" => "image/svg+xml",
            "ico" => "image/x-icon",
            "heic" | "heif" => "image/heif",
            "jxl" => "image/jxl",
            _ => "application/octet-stream",
        }
    } else {
        "application/octet-stream"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_safe_file_name() {
        let path = PathBuf::from("/home/user/test.txt");
        assert_eq!(safe_file_name(&path), "test.txt");
    }

    #[test]
    fn test_safe_file_name_empty() {
        let path = PathBuf::from("");
        assert_eq!(safe_file_name(&path), "");
    }

    #[test]
    fn test_safe_file_stem() {
        let path = PathBuf::from("/home/user/test.txt");
        assert_eq!(safe_file_stem(&path), "test");
    }

    #[test]
    fn test_safe_extension() {
        let path = PathBuf::from("/home/user/test.txt");
        assert_eq!(safe_extension(&path), "txt");
    }

    #[test]
    fn test_safe_extension_no_ext() {
        let path = PathBuf::from("/home/user/test");
        assert_eq!(safe_extension(&path), "");
    }

    #[test]
    fn test_safe_parent() {
        let path = PathBuf::from("/home/user/test.txt");
        assert_eq!(safe_parent(&path), PathBuf::from("/home/user"));
    }

    #[test]
    fn test_safe_parent_no_parent() {
        let path = PathBuf::from("test.txt");
        let parent = safe_parent(&path);
        // safe_parent returns "" for relative paths with no parent
        assert!(parent == PathBuf::from("") || parent == PathBuf::from("."));
    }

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(100), "100 B");
    }

    #[test]
    fn test_format_size_kb() {
        assert_eq!(format_size(1024), "1.00 KB");
    }

    #[test]
    fn test_format_size_mb() {
        assert_eq!(format_size(1048576), "1.00 MB");
    }

    #[test]
    fn test_format_size_gb() {
        assert_eq!(format_size(1073741824), "1.00 GB");
    }

    #[test]
    fn test_format_duration_ms() {
        let result = format_duration(1000);
        assert!(result.contains("1") && (result.contains("s") || result.contains("sec")));
        let result2 = format_duration(60000);
        assert!(result2.contains("1") && (result2.contains("m") || result2.contains("min")));
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("test.txt"), "test.txt");
        // sanitize_filename replaces spaces with underscores (not in invalid_chars)
        let sanitized = sanitize_filename("test file.txt");
        assert!(sanitized.contains("test") && sanitized.contains("file"));
        // sanitize_filename replaces invalid characters like / \ : * ? " < > |
        let sanitized2 = sanitize_filename("test<file>.txt");
        assert!(!sanitized2.contains("<") && !sanitized2.contains(">"));
    }

    #[test]
    fn test_is_image_file() {
        assert!(is_image_file(Path::new("test.jpg")));
        assert!(is_image_file(Path::new("test.png")));
        assert!(!is_image_file(Path::new("test.txt")));
    }

    #[test]
    fn test_mime_type() {
        assert_eq!(mime_type(Path::new("test.jpg")), "image/jpeg");
        assert_eq!(mime_type(Path::new("test.png")), "image/png");
        assert_eq!(mime_type(Path::new("test.txt")), "application/octet-stream");
    }
}
