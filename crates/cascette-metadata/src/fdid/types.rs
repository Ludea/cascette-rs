//! Core types and traits for FileDataID management
//!
//! This module defines the fundamental types used for FileDataID orchestration,
//! including provider traits, mapping structures, and statistics.

// Legacy types kept for compatibility
// New provider system uses types from crate::fdid::provider

// Legacy types no longer use MetadataResult, they use the new error types
use crate::fdid::provider::{FileDataIdMapping, FileDataIdProvider};
use async_trait::async_trait;
use serde::Serialize;

/// Statistics about the FileDataID service
#[derive(Debug, Serialize, Clone)]
pub struct FileDataIdStats {
    /// Total number of mappings loaded
    pub total_mappings: usize,
    /// Memory usage in bytes (approximate)
    pub memory_usage_bytes: usize,
    /// Number of ID → path lookups performed
    pub id_to_path_lookups: u64,
    /// Number of path → ID lookups performed
    pub path_to_id_lookups: u64,
    /// Number of successful lookups
    pub successful_lookups: u64,
    /// Number of failed lookups
    pub failed_lookups: u64,
    /// Time when mappings were last loaded
    pub last_loaded: Option<chrono::DateTime<chrono::Utc>>,
    /// Provider source information
    pub provider_info: String,
    /// Cache statistics
    pub cache_stats: Option<CacheMetrics>,
}

/// Cache performance metrics
#[derive(Debug, Serialize, Clone)]
pub struct CacheMetrics {
    /// Cache hit count
    pub hits: u64,
    /// Cache miss count
    pub misses: u64,
    /// Hit rate as percentage (0.0-100.0)
    pub hit_rate: f64,
    /// Number of entries currently cached
    pub cached_entries: usize,
    /// Cache size in bytes
    pub cache_size_bytes: u64,
    /// Number of cache saves
    pub saves: u64,
    /// Number of cache loads
    pub loads: u64,
    /// Number of expired entries cleaned
    pub entries_expired: u64,
    /// Number of entries evicted
    pub entries_evicted: u64,
    /// Cache error rate as percentage
    pub error_rate: f64,
    /// Last cache save timestamp
    pub last_save: Option<chrono::DateTime<chrono::Utc>>,
    /// Last cache load timestamp
    pub last_load: Option<chrono::DateTime<chrono::Utc>>,
    /// Cache directory path
    pub cache_directory: String,
    /// Cache enabled status
    pub enabled: bool,
}

impl Default for FileDataIdStats {
    fn default() -> Self {
        Self {
            total_mappings: 0,
            memory_usage_bytes: 0,
            id_to_path_lookups: 0,
            path_to_id_lookups: 0,
            successful_lookups: 0,
            failed_lookups: 0,
            last_loaded: None,
            provider_info: "None".to_string(),
            cache_stats: None,
        }
    }
}

impl Default for CacheMetrics {
    fn default() -> Self {
        Self {
            hits: 0,
            misses: 0,
            hit_rate: 0.0,
            cached_entries: 0,
            cache_size_bytes: 0,
            saves: 0,
            loads: 0,
            entries_expired: 0,
            entries_evicted: 0,
            error_rate: 0.0,
            last_save: None,
            last_load: None,
            cache_directory: String::new(),
            enabled: false,
        }
    }
}

// Legacy FileDataIdProvider trait moved to crate::fdid::provider
// Legacy ListfileProviderAdapter moved to crate::fdid::adapter

/// In-memory provider for testing and small datasets
pub struct MemoryProvider {
    mappings: Vec<FileDataIdMapping>,
    source_info: String,
}

impl MemoryProvider {
    /// Create a new memory provider with the given mappings
    pub fn new(mappings: Vec<FileDataIdMapping>) -> Self {
        Self {
            mappings,
            source_info: "MemoryProvider".to_string(),
        }
    }

    /// Create an empty memory provider
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    /// Add a mapping to this provider
    pub fn add_mapping(&mut self, mapping: FileDataIdMapping) {
        self.mappings.push(mapping);
    }

    /// Add multiple mappings to this provider
    pub fn add_mappings(&mut self, mut mappings: Vec<FileDataIdMapping>) {
        self.mappings.append(&mut mappings);
    }
}

#[async_trait]
impl FileDataIdProvider for MemoryProvider {
    fn info(&self) -> crate::fdid::provider::ProviderInfo {
        crate::fdid::provider::ProviderInfo {
            name: self.source_info.clone(),
            description: "Legacy in-memory provider".to_string(),
            version: "1.0.0".to_string(),
            source_type: crate::fdid::provider::SourceType::Memory,
            capabilities: crate::fdid::provider::ProviderCapabilities::default(),
            last_updated: Some(chrono::Utc::now()),
            metadata: std::collections::HashMap::new(),
        }
    }

    async fn initialize(&mut self) -> crate::fdid::FileDataIdResult<()> {
        Ok(())
    }

    async fn is_available(&self) -> bool {
        true
    }

    async fn resolve_id(&self, id: u32) -> crate::fdid::FileDataIdResult<Option<String>> {
        Ok(self
            .mappings
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.path.clone()))
    }

    async fn resolve_path(&self, path: &str) -> crate::fdid::FileDataIdResult<Option<u32>> {
        Ok(self.mappings.iter().find(|m| m.path == path).map(|m| m.id))
    }

    async fn get_mappings(
        &self,
        ids: &[u32],
    ) -> crate::fdid::FileDataIdResult<Vec<FileDataIdMapping>> {
        let mut result = Vec::new();
        for &id in ids {
            if let Some(mapping) = self.mappings.iter().find(|m| m.id == id) {
                result.push(mapping.clone());
            }
        }
        Ok(result)
    }

    async fn list_ids(&self) -> crate::fdid::FileDataIdResult<Vec<u32>> {
        Ok(self.mappings.iter().map(|m| m.id).collect())
    }

    async fn mapping_count(&self) -> crate::fdid::FileDataIdResult<usize> {
        Ok(self.mappings.len())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_file_data_id_mapping_creation() {
        let mapping = FileDataIdMapping::new(12345, "Interface/AddOns/Test/Test.toc".to_string());

        assert_eq!(mapping.id, 12345);
        assert_eq!(mapping.path, "Interface/AddOns/Test/Test.toc");
    }

    #[test]
    fn test_stats_default() {
        let stats = FileDataIdStats::default();

        assert_eq!(stats.total_mappings, 0);
        assert_eq!(stats.successful_lookups, 0);
        assert_eq!(stats.failed_lookups, 0);
        assert!(stats.last_loaded.is_none());
    }

    #[test]
    fn test_memory_provider() {
        let mut provider = MemoryProvider::empty();

        provider.add_mapping(FileDataIdMapping::new(
            12345,
            "Interface/Test.lua".to_string(),
        ));

        assert_eq!(provider.mappings.len(), 1);
        let info = provider.info();
        assert!(info.name.contains("MemoryProvider"));
    }

    #[tokio::test]
    async fn test_memory_provider_async() {
        let mut provider = MemoryProvider::empty();
        provider.add_mapping(FileDataIdMapping::new(
            54321,
            "Interface/Test2.lua".to_string(),
        ));

        let mappings = provider
            .get_mappings(&[54321])
            .await
            .expect("Test assertion");
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].id, 54321);

        assert!(provider.is_available().await);
        assert_eq!(provider.mapping_count().await.expect("Test assertion"), 1);
    }

    // Removed ListfileProviderAdapter test as it's now feature-gated
}
