//! FileDataID import utilities
//!
//! Simple utilities for downloading and importing FileDataID mappings
//! directly into storage, separate from the service layer.

use crate::fdid::storage::{CachedStorage, FileDataIdMappings};
use std::path::Path;
use thiserror::Error;

/// Import errors
#[derive(Error, Debug)]
pub enum ImportError {
    /// Network request failed
    #[error("Failed to download listfile: {0}")]
    NetworkError(String),
    /// File parsing failed
    #[error("Failed to parse listfile: {0}")]
    ParseError(String),
    /// Storage operation failed
    #[error("Storage error: {0}")]
    StorageError(String),
}

/// Result type for import operations
pub type ImportResult<T> = Result<T, ImportError>;

/// Simple FileDataID importer
pub struct FileDataIdImporter {
    /// HTTP client for downloads
    client: reqwest::Client,
}

impl FileDataIdImporter {
    /// Create new importer
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Download and import FileDataID mappings from WoWDev repository
    pub async fn import_from_wowdev(&self) -> ImportResult<FileDataIdMappings> {
        const LISTFILE_URL: &str = "https://github.com/wowdev/wow-listfile/releases/latest/download/community-listfile.csv";

        // Download the listfile
        let response = self
            .client
            .get(LISTFILE_URL)
            .send()
            .await
            .map_err(|e| ImportError::NetworkError(e.to_string()))?;

        let content = response
            .text()
            .await
            .map_err(|e| ImportError::NetworkError(e.to_string()))?;

        // Parse the content
        self.parse_listfile_content(&content)
    }

    /// Parse listfile content into mappings
    pub fn parse_listfile_content(&self, content: &str) -> ImportResult<FileDataIdMappings> {
        let mut mappings = FileDataIdMappings::new();
        let mut parsed_count = 0;
        let mut error_count = 0;

        for (line_num, line) in content.lines().enumerate() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Parse format: "123456;path/to/file.ext"
            match line.split_once(';') {
                Some((id_str, path)) => {
                    match id_str.trim().parse::<u32>() {
                        Ok(id) => {
                            let normalized_path = normalize_path(path.trim());
                            mappings.id_to_path.insert(id, normalized_path.clone());
                            mappings.path_to_id.insert(normalized_path, id);
                            parsed_count += 1;
                        }
                        Err(_) => {
                            error_count += 1;
                            if error_count <= 10 {
                                // Only log first 10 errors
                                eprintln!(
                                    "Warning: Invalid FileDataID '{}' on line {}",
                                    id_str,
                                    line_num + 1
                                );
                            }
                        }
                    }
                }
                None => {
                    error_count += 1;
                    if error_count <= 10 {
                        eprintln!(
                            "Warning: Invalid format '{}' on line {}",
                            line,
                            line_num + 1
                        );
                    }
                }
            }
        }

        if error_count > 10 {
            eprintln!(
                "Warning: {} additional parse errors not shown",
                error_count - 10
            );
        }

        println!(
            "Imported {} FileDataID mappings ({} errors)",
            parsed_count, error_count
        );

        Ok(mappings)
    }

    /// Import mappings from a local file
    pub async fn import_from_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> ImportResult<FileDataIdMappings> {
        let content = tokio::fs::read_to_string(path.as_ref())
            .await
            .map_err(|e| ImportError::StorageError(e.to_string()))?;

        self.parse_listfile_content(&content)
    }

    /// Create cached storage and populate it with imported data
    pub async fn create_populated_storage<P: AsRef<Path>>(
        &self,
        cache_path: Option<P>,
    ) -> ImportResult<CachedStorage> {
        // Import the mappings
        let mappings = self.import_from_wowdev().await?;

        // Create storage
        let cache_path_buf = cache_path.map(|p| p.as_ref().to_path_buf());
        let mut storage = CachedStorage::new(cache_path_buf);

        // Initialize and load data
        storage
            .initialize()
            .await
            .map_err(|e| ImportError::StorageError(e.to_string()))?;

        // Convert mappings to iterator format
        let mappings_iter = mappings.id_to_path.into_iter();

        storage
            .load_and_cache_mappings(mappings_iter)
            .await
            .map_err(|e| ImportError::StorageError(e.to_string()))?;

        Ok(storage)
    }
}

impl Default for FileDataIdImporter {
    fn default() -> Self {
        Self::new()
    }
}

/// Normalize file path for consistent storage
fn normalize_path(path: &str) -> String {
    // Convert Windows backslashes to forward slashes
    // Normalize case and remove redundant slashes
    path.replace('\\', "/")
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path() {
        assert_eq!(
            normalize_path("world\\maps\\azeroth.wmo"),
            "world/maps/azeroth.wmo"
        );
        assert_eq!(normalize_path("interface//test.lua"), "interface/test.lua");
        assert_eq!(normalize_path("/world/maps/"), "world/maps");
        assert_eq!(normalize_path("simple.txt"), "simple.txt");
    }

    #[test]
    fn test_parse_listfile_content() {
        let importer = FileDataIdImporter::new();
        let content = r"# This is a comment
123456;world/maps/azeroth.wmo
789012;creature/human/male.m2

# Another comment
345678;interface\test.lua
invalid;line;format
999999;sound/music/stormwind.ogg";

        let mappings = importer
            .parse_listfile_content(content)
            .expect("Failed to parse test listfile content");

        assert_eq!(mappings.id_to_path.len(), 4);
        assert_eq!(mappings.path_to_id.len(), 4);

        assert_eq!(
            mappings.id_to_path.get(&123_456),
            Some(&"world/maps/azeroth.wmo".to_string())
        );
        assert_eq!(
            mappings.id_to_path.get(&345_678),
            Some(&"interface/test.lua".to_string())
        );

        assert_eq!(
            mappings.path_to_id.get("world/maps/azeroth.wmo"),
            Some(&123_456)
        );
        assert_eq!(
            mappings.path_to_id.get("interface/test.lua"),
            Some(&345_678)
        );
    }
}
