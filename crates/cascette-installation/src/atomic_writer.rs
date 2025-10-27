//! Atomic file writing utilities for safe installation
//!
//! This module provides atomic file writing capabilities to prevent
//! partial file corruption during installation interruptions.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Extension used for temporary files during atomic writes
const TEMP_EXTENSION: &str = "cascette_tmp";

/// Atomically write data to a file using temp file + rename pattern
///
/// This ensures that the target file either contains complete data or
/// doesn't exist at all - never partial data.
pub fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    let temp_path = get_temp_path(path);

    // Write to temp file
    fs::write(&temp_path, data)?;

    // Atomic rename (on same filesystem)
    match fs::rename(&temp_path, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            // Clean up temp file on failure
            let _ = fs::remove_file(&temp_path);
            Err(e)
        }
    }
}

/// Get the temporary file path for atomic writes
fn get_temp_path(path: &Path) -> PathBuf {
    path.with_extension(TEMP_EXTENSION)
}

/// Clean up any orphaned temporary files in a directory
///
/// This is useful for cleaning up after crashes or interruptions
#[allow(dead_code)]
pub fn cleanup_temp_files(dir: &Path) -> io::Result<usize> {
    let mut cleaned = 0;

    if !dir.is_dir() {
        return Ok(0);
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some(TEMP_EXTENSION)
            && fs::remove_file(&path).is_ok()
        {
            cleaned += 1;
        }
    }

    Ok(cleaned)
}

/// Recursively clean up temporary files in a directory tree
pub fn cleanup_temp_files_recursive(dir: &Path) -> io::Result<usize> {
    let mut cleaned = 0;

    if !dir.is_dir() {
        return Ok(0);
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            cleaned += cleanup_temp_files_recursive(&path)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some(TEMP_EXTENSION)
            && fs::remove_file(&path).is_ok()
        {
            cleaned += 1;
        }
    }

    Ok(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_atomic_write_success() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let file_path = temp_dir.path().join("test.txt");
        let data = b"Hello, World!";

        atomic_write(&file_path, data).expect("Failed to write atomically");

        assert!(file_path.exists());
        assert_eq!(fs::read(&file_path).expect("Failed to read file"), data);

        // Ensure no temp file remains
        let temp_path = get_temp_path(&file_path);
        assert!(!temp_path.exists());
    }

    #[test]
    fn test_atomic_write_overwrites() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let file_path = temp_dir.path().join("test.txt");

        // Write initial data
        atomic_write(&file_path, b"Old data").expect("Failed to write old data");

        // Overwrite with new data
        let new_data = b"New data";
        atomic_write(&file_path, new_data).expect("Failed to write new data");

        assert_eq!(fs::read(&file_path).expect("Failed to read file"), new_data);
    }

    #[test]
    fn test_cleanup_temp_files() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Create some temporary files
        let temp_file1 = temp_dir.path().join("file1.cascette_tmp");
        let temp_file2 = temp_dir.path().join("file2.cascette_tmp");
        let normal_file = temp_dir.path().join("normal.txt");

        fs::write(&temp_file1, b"temp1").expect("Failed to write temp file");
        fs::write(&temp_file2, b"temp2").expect("Failed to write temp file");
        fs::write(&normal_file, b"normal").expect("Failed to write normal file");

        // Clean up temp files
        let cleaned = cleanup_temp_files(temp_dir.path()).expect("Failed to cleanup temp files");

        assert_eq!(cleaned, 2);
        assert!(!temp_file1.exists());
        assert!(!temp_file2.exists());
        assert!(normal_file.exists());
    }

    #[test]
    fn test_cleanup_temp_files_recursive() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let sub_dir = temp_dir.path().join("subdir");
        fs::create_dir(&sub_dir).expect("Failed to create subdirectory");

        // Create temp files at different levels
        let root_temp = temp_dir.path().join("root.cascette_tmp");
        let sub_temp = sub_dir.join("sub.cascette_tmp");
        let normal_file = temp_dir.path().join("normal.txt");

        fs::write(&root_temp, b"root").expect("Failed to write root temp file");
        fs::write(&sub_temp, b"sub").expect("Failed to write sub temp file");
        fs::write(&normal_file, b"normal").expect("Failed to write normal file");

        // Clean up recursively
        let cleaned = cleanup_temp_files_recursive(temp_dir.path())
            .expect("Failed to cleanup temp files recursively");

        assert_eq!(cleaned, 2);
        assert!(!root_temp.exists());
        assert!(!sub_temp.exists());
        assert!(normal_file.exists());
    }

    #[test]
    fn test_cleanup_empty_directory() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cleaned = cleanup_temp_files(temp_dir.path()).expect("Failed to cleanup temp files");
        assert_eq!(cleaned, 0);
    }

    #[test]
    fn test_cleanup_nonexistent_directory() {
        let result = cleanup_temp_files(Path::new("/nonexistent/path"));
        assert!(result.is_ok());
        assert_eq!(result.expect("Result should be Ok"), 0);
    }
}
