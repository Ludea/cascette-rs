//! Provider trait for FileDataID resolution backends

use crate::fdid::{FileDataIdError, FileDataIdResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single FileDataID to file path mapping
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, bincode::Encode, bincode::Decode)]
pub struct FileDataIdMapping {
    /// The numeric FileDataID
    pub id: u32,
    /// The corresponding file path (using forward slashes)
    pub path: String,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl FileDataIdMapping {
    /// Create a new FileDataID mapping
    pub fn new(id: u32, path: String) -> Self {
        Self {
            id,
            path,
            metadata: HashMap::new(),
        }
    }
}
/// Trait for FileDataID data providers
///
/// This trait defines the interface that all FileDataID providers must implement.
/// Providers can source data from various backends such as community listfiles,
/// local databases, API endpoints, or cached files.
///
/// The trait follows the established pattern from `TactKeyProvider` and `ImportProvider`,
/// providing both mandatory operations and optional optimizations that providers can override.
#[async_trait]
pub trait FileDataIdProvider: Send + Sync {
    /// Get provider information and capabilities
    fn info(&self) -> ProviderInfo;

    /// Initialize the provider (load data, authenticate, validate config, etc.)
    async fn initialize(&mut self) -> FileDataIdResult<()>;

    /// Check if the provider is currently available and functional
    async fn is_available(&self) -> bool;

    /// Resolve a FileDataID to its corresponding file path
    ///
    /// This is the core operation that most providers must implement efficiently.
    /// Returns `None` if the ID is not found in the provider's dataset.
    async fn resolve_id(&self, id: u32) -> FileDataIdResult<Option<String>>;

    /// Resolve a file path to its corresponding FileDataID
    ///
    /// This is the reverse lookup operation. Some providers may implement this
    /// more efficiently than others depending on their internal data structures.
    /// Returns `None` if the path is not found in the provider's dataset.
    async fn resolve_path(&self, path: &str) -> FileDataIdResult<Option<u32>>;

    /// Get a complete mapping entry for a FileDataID
    ///
    /// The default implementation uses `resolve_id` and constructs a basic mapping.
    /// Providers can override this to return additional metadata.
    async fn get_mapping(&self, id: u32) -> FileDataIdResult<Option<FileDataIdMapping>> {
        if let Some(path) = self.resolve_id(id).await? {
            Ok(Some(FileDataIdMapping::new(id, path)))
        } else {
            Ok(None)
        }
    }

    /// Get multiple mappings efficiently
    ///
    /// The default implementation calls `get_mapping` for each ID sequentially.
    /// Providers should override this for batch operations when possible.
    async fn get_mappings(&self, ids: &[u32]) -> FileDataIdResult<Vec<FileDataIdMapping>> {
        let mut mappings = Vec::new();

        for &id in ids {
            if let Some(mapping) = self.get_mapping(id).await? {
                mappings.push(mapping);
            }
        }

        Ok(mappings)
    }

    /// Search for mappings matching the given criteria
    ///
    /// The default implementation is inefficient as it may need to iterate
    /// over all mappings. Providers should override this when they can
    /// perform indexed searches.
    async fn search_mappings(
        &self,
        query: &FileDataIdQuery,
    ) -> FileDataIdResult<Vec<FileDataIdMapping>> {
        // Default implementation - providers should override for efficiency
        let _ = query;
        Err(FileDataIdError::Generic(
            "Search not implemented by provider".to_string(),
        ))
    }

    /// Get all available FileDataIDs
    ///
    /// This is an expensive operation that should be used sparingly.
    /// Returns an empty vector if the provider doesn't support enumeration.
    async fn list_ids(&self) -> FileDataIdResult<Vec<u32>> {
        // Default implementation returns empty - providers override if supported
        Ok(Vec::new())
    }

    /// Get the total number of mappings available
    ///
    /// Returns 0 if the provider doesn't know or can't efficiently count.
    async fn mapping_count(&self) -> FileDataIdResult<usize> {
        // Default implementation counts list_ids - providers can optimize
        Ok(self.list_ids().await?.len())
    }

    /// Check if a FileDataID exists in the provider's dataset
    async fn contains_id(&self, id: u32) -> FileDataIdResult<bool> {
        Ok(self.resolve_id(id).await?.is_some())
    }

    /// Check if a file path exists in the provider's dataset
    async fn contains_path(&self, path: &str) -> FileDataIdResult<bool> {
        Ok(self.resolve_path(path).await?.is_some())
    }

    /// Refresh the provider's data from its source
    ///
    /// This may involve re-downloading files, refreshing caches, or
    /// reloading from disk. The default implementation does nothing.
    async fn refresh(&mut self) -> FileDataIdResult<usize> {
        Ok(0)
    }

    /// Get provider statistics and performance metrics
    async fn stats(&self) -> FileDataIdResult<ResolutionStats> {
        Ok(ResolutionStats::default())
    }

    /// Get provider configuration information
    fn config(&self) -> &dyn ProviderConfig {
        &DefaultProviderConfig
    }
}

/// Provider information and metadata
#[derive(Debug, Clone)]
pub struct ProviderInfo {
    /// Provider name (e.g., "WoWDev Listfile", "Local Cache")
    pub name: String,

    /// Provider description
    pub description: String,

    /// Provider version
    pub version: String,

    /// Data source type
    pub source_type: SourceType,

    /// Supported operations
    pub capabilities: ProviderCapabilities,

    /// Data freshness indicator
    pub last_updated: Option<chrono::DateTime<chrono::Utc>>,

    /// Provider-specific metadata
    pub metadata: std::collections::HashMap<String, String>,
}

/// Type of data source
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceType {
    /// Community-maintained listfile
    Listfile,
    /// Local database or cache
    Local,
    /// Remote API endpoint
    Remote,
    /// In-memory dataset
    Memory,
    /// Custom provider type
    Custom,
}

impl SourceType {
    /// Get display name for the source type
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Listfile => "Community Listfile",
            Self::Local => "Local Storage",
            Self::Remote => "Remote API",
            Self::Memory => "In-Memory",
            Self::Custom => "Custom",
        }
    }
}

/// Provider capabilities
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct ProviderCapabilities {
    /// Supports ID to path resolution
    pub id_to_path: bool,
    /// Supports path to ID resolution
    pub path_to_id: bool,
    /// Supports search operations
    pub search: bool,
    /// Supports enumeration (listing all IDs)
    pub enumeration: bool,
    /// Supports batch operations
    pub batch_operations: bool,
    /// Supports real-time updates
    pub real_time_updates: bool,
    /// Maximum batch size (None = no limit)
    pub max_batch_size: Option<usize>,
}

impl Default for ProviderCapabilities {
    fn default() -> Self {
        Self {
            id_to_path: true,
            path_to_id: true,
            search: false,
            enumeration: false,
            batch_operations: false,
            real_time_updates: false,
            max_batch_size: None,
        }
    }
}

/// Configuration interface for providers
pub trait ProviderConfig: Send + Sync {
    /// Get configuration as key-value pairs
    fn as_map(&self) -> std::collections::HashMap<String, String>;

    /// Validate the current configuration
    fn validate(&self) -> FileDataIdResult<()>;
}

/// Default empty configuration
struct DefaultProviderConfig;

impl ProviderConfig for DefaultProviderConfig {
    fn as_map(&self) -> std::collections::HashMap<String, String> {
        std::collections::HashMap::new()
    }

    fn validate(&self) -> FileDataIdResult<()> {
        Ok(())
    }
}

/// Unified provider wrapper that can use any backend
///
/// This follows the same pattern as `UnifiedKeyStore` from the TACT key system,
/// providing a consistent interface while allowing different provider implementations.
#[derive(Debug)]
pub struct UnifiedFileDataIdProvider<T: FileDataIdProvider> {
    provider: T,
}

impl<T: FileDataIdProvider> UnifiedFileDataIdProvider<T> {
    /// Create a new unified provider with the specified backend
    pub fn new(provider: T) -> Self {
        Self { provider }
    }

    /// Get the underlying provider
    pub fn provider(&self) -> &T {
        &self.provider
    }

    /// Get mutable access to the underlying provider
    pub fn provider_mut(&mut self) -> &mut T {
        &mut self.provider
    }

    /// Consume the unified provider and return the backend
    pub fn into_provider(self) -> T {
        self.provider
    }
}

#[async_trait]
impl<T: FileDataIdProvider> FileDataIdProvider for UnifiedFileDataIdProvider<T> {
    fn info(&self) -> ProviderInfo {
        self.provider.info()
    }

    async fn initialize(&mut self) -> FileDataIdResult<()> {
        self.provider.initialize().await
    }

    async fn is_available(&self) -> bool {
        self.provider.is_available().await
    }

    async fn resolve_id(&self, id: u32) -> FileDataIdResult<Option<String>> {
        self.provider.resolve_id(id).await
    }

    async fn resolve_path(&self, path: &str) -> FileDataIdResult<Option<u32>> {
        self.provider.resolve_path(path).await
    }

    async fn get_mapping(&self, id: u32) -> FileDataIdResult<Option<FileDataIdMapping>> {
        self.provider.get_mapping(id).await
    }

    async fn get_mappings(&self, ids: &[u32]) -> FileDataIdResult<Vec<FileDataIdMapping>> {
        self.provider.get_mappings(ids).await
    }

    async fn search_mappings(
        &self,
        query: &FileDataIdQuery,
    ) -> FileDataIdResult<Vec<FileDataIdMapping>> {
        self.provider.search_mappings(query).await
    }

    async fn list_ids(&self) -> FileDataIdResult<Vec<u32>> {
        self.provider.list_ids().await
    }

    async fn mapping_count(&self) -> FileDataIdResult<usize> {
        self.provider.mapping_count().await
    }

    async fn contains_id(&self, id: u32) -> FileDataIdResult<bool> {
        self.provider.contains_id(id).await
    }

    async fn contains_path(&self, path: &str) -> FileDataIdResult<bool> {
        self.provider.contains_path(path).await
    }

    async fn refresh(&mut self) -> FileDataIdResult<usize> {
        self.provider.refresh().await
    }

    async fn stats(&self) -> FileDataIdResult<ResolutionStats> {
        self.provider.stats().await
    }

    fn config(&self) -> &dyn ProviderConfig {
        self.provider.config()
    }
}

/// File data ID query for searching mappings
#[derive(Debug, Clone, Default)]
pub struct FileDataIdQuery {
    /// Path pattern to match (supports wildcards)
    pub path_pattern: Option<String>,
    /// Filename pattern
    pub filename_pattern: Option<String>,
    /// ID range filter
    pub id_range: Option<(u32, u32)>,
}

impl FileDataIdQuery {
    /// Create a new empty query
    pub fn new() -> Self {
        Self::default()
    }

    /// Set path pattern filter
    #[must_use]
    pub fn with_path_pattern(mut self, pattern: String) -> Self {
        self.path_pattern = Some(pattern);
        self
    }

    /// Set filename pattern filter
    #[must_use]
    pub fn with_filename_pattern(mut self, pattern: String) -> Self {
        self.filename_pattern = Some(pattern);
        self
    }

    /// Set ID range filter
    #[must_use]
    pub fn with_id_range(mut self, min_id: u32, max_id: u32) -> Self {
        self.id_range = Some((min_id, max_id));
        self
    }

    /// Check if a mapping matches this query
    pub fn matches(&self, mapping: &FileDataIdMapping) -> bool {
        // ID range check
        if let Some((min_id, max_id)) = self.id_range {
            if mapping.id < min_id || mapping.id > max_id {
                return false;
            }
        }

        // Path pattern check
        if let Some(ref pattern) = self.path_pattern {
            if !matches_pattern(&mapping.path, pattern) {
                return false;
            }
        }

        // Filename pattern check
        if let Some(ref pattern) = self.filename_pattern {
            let filename = std::path::Path::new(&mapping.path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&mapping.path);
            if !matches_pattern(filename, pattern) {
                return false;
            }
        }

        true
    }
}

/// Resolution statistics for tracking provider performance
#[derive(Debug, Clone, Default)]
pub struct ResolutionStats {
    /// Total number of mappings available
    pub total_mappings: usize,
    /// ID to path lookup hits
    pub id_to_path_hits: u64,
    /// ID to path lookup misses
    pub id_to_path_misses: u64,
    /// Path to ID lookup hits
    pub path_to_id_hits: u64,
    /// Path to ID lookup misses
    pub path_to_id_misses: u64,
    /// ID cache hits
    pub id_cache_hits: u64,
    /// ID cache misses
    pub id_cache_misses: u64,
    /// Path cache hits
    pub path_cache_hits: u64,
    /// Path cache misses
    pub path_cache_misses: u64,
    /// Memory usage in bytes
    pub memory_usage_bytes: usize,
    /// Last update timestamp
    pub last_updated: Option<chrono::DateTime<chrono::Utc>>,
}

impl ResolutionStats {
    /// Calculate overall hit rate as percentage
    pub fn hit_rate(&self) -> f64 {
        let total_hits = self.id_to_path_hits + self.path_to_id_hits;
        let total_requests = total_hits + self.id_to_path_misses + self.path_to_id_misses;

        if total_requests == 0 {
            0.0
        } else {
            (total_hits as f64 / total_requests as f64) * 100.0
        }
    }

    /// Calculate cache hit rate as percentage
    pub fn cache_hit_rate(&self) -> f64 {
        let total_cache_hits = self.id_cache_hits + self.path_cache_hits;
        let total_cache_requests = total_cache_hits + self.id_cache_misses + self.path_cache_misses;

        if total_cache_requests == 0 {
            0.0
        } else {
            (total_cache_hits as f64 / total_cache_requests as f64) * 100.0
        }
    }
}

/// Basic pattern matching with wildcard support
fn matches_pattern(text: &str, pattern: &str) -> bool {
    // Simple wildcard matching - * matches any sequence of characters
    if pattern == "*" {
        return true;
    }

    if !pattern.contains('*') {
        return text == pattern;
    }

    // Split pattern by asterisks and check each part
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.is_empty() {
        return true;
    }

    let mut pos = 0;

    // Check first part (must be at the beginning)
    if !parts[0].is_empty() {
        if !text.starts_with(parts[0]) {
            return false;
        }
        pos += parts[0].len();
    }

    // Check middle parts
    for part in &parts[1..parts.len() - 1] {
        if part.is_empty() {
            continue;
        }

        if let Some(found_pos) = text[pos..].find(part) {
            pos += found_pos + part.len();
        } else {
            return false;
        }
    }

    // Check last part (must be at the end)
    if let Some(last_part) = parts.last() {
        if !last_part.is_empty() {
            return text[pos..].ends_with(last_part);
        }
    }

    true
}

/// In-memory provider for testing and development
#[derive(Debug)]
pub struct MemoryProvider {
    mappings: Vec<FileDataIdMapping>,
    info: ProviderInfo,
}

impl MemoryProvider {
    /// Create a new memory provider with the given mappings
    pub fn new(mappings: Vec<FileDataIdMapping>) -> Self {
        let info = ProviderInfo {
            name: "Memory Provider".to_string(),
            description: "In-memory FileDataID provider for testing".to_string(),
            version: "1.0.0".to_string(),
            source_type: SourceType::Memory,
            capabilities: ProviderCapabilities {
                id_to_path: true,
                path_to_id: true,
                search: true,
                enumeration: true,
                batch_operations: true,
                real_time_updates: false,
                max_batch_size: None,
            },
            last_updated: Some(chrono::Utc::now()),
            metadata: {
                let mut metadata = HashMap::new();
                metadata.insert("mapping_count".to_string(), mappings.len().to_string());
                metadata
            },
        };

        Self { mappings, info }
    }

    /// Create an empty memory provider
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    /// Add a mapping to this provider
    pub fn add_mapping(&mut self, mapping: FileDataIdMapping) {
        self.mappings.push(mapping);
        // Update metadata
        self.info
            .metadata
            .insert("mapping_count".to_string(), self.mappings.len().to_string());
        self.info.last_updated = Some(chrono::Utc::now());
    }

    /// Add multiple mappings to this provider
    pub fn add_mappings(&mut self, mut mappings: Vec<FileDataIdMapping>) {
        self.mappings.append(&mut mappings);
        // Update metadata
        self.info
            .metadata
            .insert("mapping_count".to_string(), self.mappings.len().to_string());
        self.info.last_updated = Some(chrono::Utc::now());
    }

    /// Get all mappings (for testing)
    pub fn mappings(&self) -> &[FileDataIdMapping] {
        &self.mappings
    }
}

#[async_trait]
impl FileDataIdProvider for MemoryProvider {
    fn info(&self) -> ProviderInfo {
        self.info.clone()
    }

    async fn initialize(&mut self) -> FileDataIdResult<()> {
        Ok(())
    }

    async fn is_available(&self) -> bool {
        true
    }

    async fn resolve_id(&self, id: u32) -> FileDataIdResult<Option<String>> {
        Ok(self
            .mappings
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.path.clone()))
    }

    async fn resolve_path(&self, path: &str) -> FileDataIdResult<Option<u32>> {
        Ok(self.mappings.iter().find(|m| m.path == path).map(|m| m.id))
    }

    async fn get_mapping(&self, id: u32) -> FileDataIdResult<Option<FileDataIdMapping>> {
        Ok(self.mappings.iter().find(|m| m.id == id).cloned())
    }

    async fn get_mappings(&self, ids: &[u32]) -> FileDataIdResult<Vec<FileDataIdMapping>> {
        let mut result = Vec::new();
        for &id in ids {
            if let Some(mapping) = self.mappings.iter().find(|m| m.id == id) {
                result.push(mapping.clone());
            }
        }
        Ok(result)
    }

    async fn search_mappings(
        &self,
        query: &FileDataIdQuery,
    ) -> FileDataIdResult<Vec<FileDataIdMapping>> {
        Ok(self
            .mappings
            .iter()
            .filter(|mapping| query.matches(mapping))
            .cloned()
            .collect())
    }

    async fn list_ids(&self) -> FileDataIdResult<Vec<u32>> {
        Ok(self.mappings.iter().map(|m| m.id).collect())
    }

    async fn mapping_count(&self) -> FileDataIdResult<usize> {
        Ok(self.mappings.len())
    }

    async fn contains_id(&self, id: u32) -> FileDataIdResult<bool> {
        Ok(self.mappings.iter().any(|m| m.id == id))
    }

    async fn contains_path(&self, path: &str) -> FileDataIdResult<bool> {
        Ok(self.mappings.iter().any(|m| m.path == path))
    }

    async fn refresh(&mut self) -> FileDataIdResult<usize> {
        // Memory provider doesn't need refresh
        Ok(self.mappings.len())
    }

    async fn stats(&self) -> FileDataIdResult<ResolutionStats> {
        Ok(ResolutionStats {
            total_mappings: self.mappings.len(),
            memory_usage_bytes: self.mappings.len() * std::mem::size_of::<FileDataIdMapping>(),
            last_updated: self.info.last_updated,
            ..Default::default()
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_provider_basic_operations() {
        let mut provider = MemoryProvider::empty();

        // Test initial state
        assert!(provider.is_available().await);
        assert_eq!(provider.mapping_count().await.expect("Test assertion"), 0);

        // Add a mapping
        let mapping = FileDataIdMapping::new(12345, "Interface/Test.lua".to_string());
        provider.add_mapping(mapping);

        // Test resolution
        let path = provider.resolve_id(12345).await.expect("Test assertion");
        assert_eq!(path, Some("Interface/Test.lua".to_string()));

        let id = provider
            .resolve_path("Interface/Test.lua")
            .await
            .expect("Test assertion");
        assert_eq!(id, Some(12345));

        // Test mapping retrieval
        let retrieved = provider
            .get_mapping(12345)
            .await
            .expect("Test assertion")
            .expect("Test assertion");
        assert_eq!(retrieved.id, 12345);
        assert_eq!(retrieved.path, "Interface/Test.lua");
    }

    #[tokio::test]
    async fn test_memory_provider_batch_operations() {
        let mut provider = MemoryProvider::empty();

        // Add multiple mappings
        provider.add_mapping(FileDataIdMapping::new(100, "File1.txt".to_string()));
        provider.add_mapping(FileDataIdMapping::new(200, "File2.txt".to_string()));
        provider.add_mapping(FileDataIdMapping::new(300, "File3.txt".to_string()));

        // Test batch retrieval
        let ids = vec![100, 200, 999]; // 999 doesn't exist
        let mappings = provider.get_mappings(&ids).await.expect("Test assertion");
        assert_eq!(mappings.len(), 2);

        // Test enumeration
        let all_ids = provider.list_ids().await.expect("Test assertion");
        assert_eq!(all_ids.len(), 3);
        assert!(all_ids.contains(&100));
        assert!(all_ids.contains(&200));
        assert!(all_ids.contains(&300));
    }

    #[tokio::test]
    async fn test_file_data_id_query_matching() {
        let mapping = FileDataIdMapping::new(12345, "Interface/AddOns/Test/test.lua".to_string());

        // Test path pattern matching
        let query = FileDataIdQuery::new().with_path_pattern("Interface/AddOns/*".to_string());
        assert!(query.matches(&mapping));

        let query = FileDataIdQuery::new().with_path_pattern("World/*".to_string());
        assert!(!query.matches(&mapping));

        // Test filename pattern matching
        let query = FileDataIdQuery::new().with_filename_pattern("*.lua".to_string());
        assert!(query.matches(&mapping));

        let query = FileDataIdQuery::new().with_filename_pattern("*.txt".to_string());
        assert!(!query.matches(&mapping));

        // Test ID range matching
        let query = FileDataIdQuery::new().with_id_range(10000, 15000);
        assert!(query.matches(&mapping));

        let query = FileDataIdQuery::new().with_id_range(20000, 25000);
        assert!(!query.matches(&mapping));
    }

    #[test]
    fn test_pattern_matching() {
        assert!(matches_pattern("test.lua", "*.lua"));
        assert!(matches_pattern("Interface/Test.lua", "Interface/*"));
        assert!(matches_pattern(
            "Interface/AddOns/Test.lua",
            "Interface/*/Test.lua"
        ));
        assert!(!matches_pattern("test.txt", "*.lua"));
        assert!(!matches_pattern("World/Test.lua", "Interface/*"));
    }
}
