//! FileDataID storage abstraction
//!
//! Provides simple, focused storage interface for FileDataID mappings.
//! Integrates with cascette-cache for persistent and in-memory caching.
//! Separates data access from import/caching concerns.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Storage errors
#[derive(Error, Debug)]
pub enum StorageError {
    /// FileDataID was not found in storage
    #[error("FileDataID {0} not found")]
    IdNotFound(u32),
    /// File path was not found in storage
    #[error("Path '{0}' not found")]
    PathNotFound(String),
    /// General storage operation failure
    #[error("Storage operation failed: {0}")]
    OperationFailed(String),
}

/// Result type for storage operations
pub type StorageResult<T> = Result<T, StorageError>;

/// Simple storage abstraction for FileDataID mappings
pub trait FileDataIdStorage: Send + Sync {
    /// Get file path for a given FileDataID
    fn get_path(&self, id: u32) -> Option<&str>;

    /// Get FileDataID for a given file path
    fn get_id(&self, path: &str) -> Option<u32>;

    /// Get total number of mappings
    fn mapping_count(&self) -> usize;

    /// Check if storage is empty
    fn is_empty(&self) -> bool {
        self.mapping_count() == 0
    }

    /// Iterate over all mappings
    fn iter_mappings(&self) -> Box<dyn Iterator<Item = (u32, &str)> + '_>;

    /// Get all FileDataIDs
    fn iter_ids(&self) -> Box<dyn Iterator<Item = u32> + '_>;

    /// Get all file paths
    fn iter_paths(&self) -> Box<dyn Iterator<Item = &str> + '_>;
}

/// Cached FileDataID mappings using cascette-cache
#[derive(Serialize, Deserialize, Clone, bincode::Encode, bincode::Decode)]
pub struct FileDataIdMappings {
    /// Map from FileDataID to file path
    pub id_to_path: HashMap<u32, String>,
    /// Map from file path to FileDataID
    pub path_to_id: HashMap<String, u32>,
}

/// File-cached storage implementation
pub struct CachedStorage {
    mappings: FileDataIdMappings,
    cache_path: Option<std::path::PathBuf>,
}

/// In-memory HashMap-based storage implementation (for testing/simple cases)
pub struct HashMapStorage {
    id_to_path: HashMap<u32, String>,
    path_to_id: HashMap<String, u32>,
}

impl HashMapStorage {
    /// Create new empty storage
    pub fn new() -> Self {
        Self {
            id_to_path: HashMap::new(),
            path_to_id: HashMap::new(),
        }
    }

    /// Create storage with initial capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            id_to_path: HashMap::with_capacity(capacity),
            path_to_id: HashMap::with_capacity(capacity),
        }
    }

    /// Load mappings from iterator
    /// Load mappings from an iterator, replacing existing content
    pub fn load_mappings<I>(&mut self, mappings: I)
    where
        I: IntoIterator<Item = (u32, String)>,
    {
        self.id_to_path.clear();
        self.path_to_id.clear();

        for (id, path) in mappings {
            self.id_to_path.insert(id, path.clone());
            self.path_to_id.insert(path, id);
        }
    }

    /// Add a single mapping
    pub fn add_mapping(&mut self, id: u32, path: String) {
        // Remove old mapping if ID already exists
        if let Some(old_path) = self.id_to_path.remove(&id) {
            self.path_to_id.remove(&old_path);
        }

        // Remove old mapping if path already exists
        if let Some(old_id) = self.path_to_id.remove(&path) {
            self.id_to_path.remove(&old_id);
        }

        // Add new mapping
        self.id_to_path.insert(id, path.clone());
        self.path_to_id.insert(path, id);
    }

    /// Get memory usage estimate in bytes
    /// Estimate memory usage in bytes
    pub fn memory_usage(&self) -> usize {
        let id_map_size =
            self.id_to_path.len() * (std::mem::size_of::<u32>() + std::mem::size_of::<String>());
        let path_map_size =
            self.path_to_id.len() * (std::mem::size_of::<String>() + std::mem::size_of::<u32>());

        // Add estimated string storage (rough estimate)
        let string_storage: usize = self.id_to_path.values().map(String::len).sum();

        id_map_size + path_map_size + (string_storage * 2) // Strings stored twice
    }
}

impl FileDataIdMappings {
    /// Create new empty mappings
    pub fn new() -> Self {
        Self {
            id_to_path: HashMap::new(),
            path_to_id: HashMap::new(),
        }
    }

    /// Load mappings from an iterator, replacing existing content
    pub fn load_mappings<I>(&mut self, mappings: I)
    where
        I: IntoIterator<Item = (u32, String)>,
    {
        self.id_to_path.clear();
        self.path_to_id.clear();

        for (id, path) in mappings {
            self.id_to_path.insert(id, path.clone());
            self.path_to_id.insert(path, id);
        }
    }

    /// Estimate memory usage in bytes
    pub fn memory_usage(&self) -> usize {
        let id_map_size =
            self.id_to_path.len() * (std::mem::size_of::<u32>() + std::mem::size_of::<String>());
        let path_map_size =
            self.path_to_id.len() * (std::mem::size_of::<String>() + std::mem::size_of::<u32>());
        let string_storage: usize = self.id_to_path.values().map(String::len).sum();
        id_map_size + path_map_size + (string_storage * 2)
    }
}

impl CachedStorage {
    /// Create new cached storage with optional cache file
    pub fn new(cache_path: Option<std::path::PathBuf>) -> Self {
        Self {
            mappings: FileDataIdMappings::new(),
            cache_path,
        }
    }

    /// Load mappings from cache file if it exists
    pub async fn initialize(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(ref path) = self.cache_path {
            if path.exists() {
                let data = tokio::fs::read(path).await?;
                let config = bincode::config::standard();
                self.mappings = bincode::decode_from_slice(&data, config)?.0;
            }
        }
        Ok(())
    }

    /// Load mappings and save to cache file
    pub async fn load_and_cache_mappings<I>(
        &mut self,
        mappings: I,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        I: IntoIterator<Item = (u32, String)>,
    {
        self.mappings.load_mappings(mappings);

        // Save to cache file if path is set
        if let Some(ref path) = self.cache_path {
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let config = bincode::config::standard();
            let data = bincode::encode_to_vec(&self.mappings, config)?;
            tokio::fs::write(path, data).await?;
        }

        Ok(())
    }

    /// Get memory usage
    pub fn total_memory_usage(&self) -> usize {
        self.mappings.memory_usage()
    }
}

impl Default for FileDataIdMappings {
    fn default() -> Self {
        Self::new()
    }
}

impl FileDataIdStorage for CachedStorage {
    fn get_path(&self, id: u32) -> Option<&str> {
        self.mappings.id_to_path.get(&id).map(String::as_str)
    }

    fn get_id(&self, path: &str) -> Option<u32> {
        self.mappings.path_to_id.get(path).copied()
    }

    fn mapping_count(&self) -> usize {
        self.mappings.id_to_path.len()
    }

    fn iter_mappings(&self) -> Box<dyn Iterator<Item = (u32, &str)> + '_> {
        Box::new(
            self.mappings
                .id_to_path
                .iter()
                .map(|(id, path)| (*id, path.as_str())),
        )
    }

    fn iter_ids(&self) -> Box<dyn Iterator<Item = u32> + '_> {
        Box::new(self.mappings.id_to_path.keys().copied())
    }

    fn iter_paths(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        Box::new(self.mappings.id_to_path.values().map(String::as_str))
    }
}

impl Default for HashMapStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl FileDataIdStorage for HashMapStorage {
    fn get_path(&self, id: u32) -> Option<&str> {
        self.id_to_path.get(&id).map(String::as_str)
    }

    fn get_id(&self, path: &str) -> Option<u32> {
        self.path_to_id.get(path).copied()
    }

    fn mapping_count(&self) -> usize {
        self.id_to_path.len()
    }

    fn iter_mappings(&self) -> Box<dyn Iterator<Item = (u32, &str)> + '_> {
        Box::new(
            self.id_to_path
                .iter()
                .map(|(id, path)| (*id, path.as_str())),
        )
    }

    fn iter_ids(&self) -> Box<dyn Iterator<Item = u32> + '_> {
        Box::new(self.id_to_path.keys().copied())
    }

    fn iter_paths(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        Box::new(self.id_to_path.values().map(String::as_str))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hashmap_storage_basic_operations() {
        let mut storage = HashMapStorage::new();

        // Initially empty
        assert!(storage.is_empty());
        assert_eq!(storage.mapping_count(), 0);
        assert!(storage.get_path(123).is_none());
        assert!(storage.get_id("test.txt").is_none());

        // Add mapping
        storage.add_mapping(123, "interface/test.lua".to_string());

        assert!(!storage.is_empty());
        assert_eq!(storage.mapping_count(), 1);
        assert_eq!(storage.get_path(123), Some("interface/test.lua"));
        assert_eq!(storage.get_id("interface/test.lua"), Some(123));
    }

    #[test]
    fn test_hashmap_storage_load_mappings() {
        let mut storage = HashMapStorage::new();

        let mappings = vec![
            (1, "world/maps/azeroth.wmo".to_string()),
            (2, "creature/human/male.m2".to_string()),
            (3, "sound/music/stormwind.mp3".to_string()),
        ];

        storage.load_mappings(mappings);

        assert_eq!(storage.mapping_count(), 3);
        assert_eq!(storage.get_path(1), Some("world/maps/azeroth.wmo"));
        assert_eq!(storage.get_path(2), Some("creature/human/male.m2"));
        assert_eq!(storage.get_path(3), Some("sound/music/stormwind.mp3"));

        assert_eq!(storage.get_id("world/maps/azeroth.wmo"), Some(1));
        assert_eq!(storage.get_id("creature/human/male.m2"), Some(2));
        assert_eq!(storage.get_id("sound/music/stormwind.mp3"), Some(3));
    }

    #[test]
    fn test_hashmap_storage_iteration() {
        let mut storage = HashMapStorage::new();

        storage.add_mapping(1, "file1.txt".to_string());
        storage.add_mapping(2, "file2.txt".to_string());
        storage.add_mapping(3, "file3.txt".to_string());

        // Test ID iteration
        let ids: Vec<u32> = storage.iter_ids().collect();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
        assert!(ids.contains(&3));

        // Test path iteration
        let paths: Vec<&str> = storage.iter_paths().collect();
        assert_eq!(paths.len(), 3);
        assert!(paths.contains(&"file1.txt"));
        assert!(paths.contains(&"file2.txt"));
        assert!(paths.contains(&"file3.txt"));

        // Test mapping iteration
        assert_eq!(storage.iter_mappings().count(), 3);
    }

    #[test]
    fn test_hashmap_storage_replacement() {
        let mut storage = HashMapStorage::new();

        // Add initial mapping
        storage.add_mapping(123, "old_file.txt".to_string());
        assert_eq!(storage.mapping_count(), 1);

        // Replace with same ID, different path
        storage.add_mapping(123, "new_file.txt".to_string());
        assert_eq!(storage.mapping_count(), 1);
        assert_eq!(storage.get_path(123), Some("new_file.txt"));
        assert!(storage.get_id("old_file.txt").is_none());
        assert_eq!(storage.get_id("new_file.txt"), Some(123));
    }
}
