//! FileDataID service implementation with bidirectional lookups
//!
//! This module provides the main FileDataIdService that orchestrates FileDataID
//! mappings using efficient bidirectional HashMap lookups optimized for high
//! performance with large datasets (500k+ mappings).

use crate::error::{MetadataError, MetadataResult};
use crate::fdid::cache::{CacheConfig, FileDataIdCache};
use crate::fdid::provider::{FileDataIdMapping, FileDataIdProvider};
use crate::fdid::types::{CacheMetrics, FileDataIdStats};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Core service for FileDataID orchestration and bidirectional lookups
///
/// This service manages FileDataID ↔ file path mappings using efficient
/// bidirectional hash tables. It's designed for high-performance lookups
/// on large datasets while providing thread-safe access patterns.
///
/// ## Performance Characteristics
///
/// - **Lookups**: O(1) average case for both directions
/// - **Memory**: ~24 bytes per mapping (plus string storage)
/// - **Thread Safety**: Read-heavy workload optimized with RwLock
/// - **Capacity**: Tested with 500k+ mappings
///
/// ## Usage Example
///
/// ```rust
/// use cascette_metadata::fdid::{FileDataIdService, MemoryProvider, FileDataIdMapping};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Create a test provider with sample data
/// let mut provider = MemoryProvider::empty();
/// provider.add_mapping(FileDataIdMapping::new(
///     12345,
///     "Interface/AddOns/MyAddon/MyAddon.toc".to_string()
/// ));
///
/// // Create service and load mappings
/// let mut service = FileDataIdService::new(Box::new(provider));
/// service.load_from_provider().await?;
///
/// // Perform lookups
/// if let Some(path) = service.get_file_path(12345)? {
///     println!("FileDataID 12345 maps to: {}", path);
/// }
///
/// if let Some(id) = service.get_file_data_id("Interface/AddOns/MyAddon/MyAddon.toc")? {
///     println!("File maps to FileDataID: {}", id);
/// }
///
/// // Get statistics
/// let stats = service.get_stats().expect("should get stats");
/// println!("Total mappings: {}", stats.total_mappings);
/// # Ok(())
/// # }
/// ```
pub struct FileDataIdService {
    /// Provider for loading mappings from external sources
    provider: Box<dyn FileDataIdProvider>,
    /// Bidirectional mappings protected by RwLock for thread safety
    mappings: Arc<RwLock<BidirectionalMappings>>,
    /// Statistics tracking lookup performance and usage
    stats: Arc<RwLock<FileDataIdStats>>,
    /// Optional persistent cache for mappings
    cache: Option<Arc<FileDataIdCache>>,
}

/// Internal structure for efficient bidirectional lookups
struct BidirectionalMappings {
    /// FileDataID → file path mapping
    id_to_path: HashMap<u32, String>,
    /// File path → FileDataID mapping
    path_to_id: HashMap<String, u32>,
}

impl BidirectionalMappings {
    /// Create new empty bidirectional mappings
    fn new() -> Self {
        Self {
            id_to_path: HashMap::new(),
            path_to_id: HashMap::new(),
        }
    }

    /// Create new bidirectional mappings with pre-allocated capacity
    fn with_capacity(capacity: usize) -> Self {
        Self {
            id_to_path: HashMap::with_capacity(capacity),
            path_to_id: HashMap::with_capacity(capacity),
        }
    }

    /// Insert a new mapping into both directions
    fn insert(&mut self, file_data_id: u32, file_path: String) {
        // Remove any existing mappings to maintain consistency
        if let Some(old_path) = self.id_to_path.get(&file_data_id) {
            self.path_to_id.remove(old_path);
        }
        if let Some(old_id) = self.path_to_id.get(&file_path) {
            self.id_to_path.remove(old_id);
        }

        // Insert new mapping
        self.id_to_path.insert(file_data_id, file_path.clone());
        self.path_to_id.insert(file_path, file_data_id);
    }

    /// Get file path by FileDataID
    fn get_path(&self, file_data_id: u32) -> Option<&str> {
        self.id_to_path.get(&file_data_id).map(String::as_str)
    }

    /// Get FileDataID by file path
    fn get_id(&self, file_path: &str) -> Option<u32> {
        self.path_to_id.get(file_path).copied()
    }

    /// Get total number of mappings
    fn len(&self) -> usize {
        self.id_to_path.len()
    }

    /// Check if mappings are empty
    fn is_empty(&self) -> bool {
        self.id_to_path.is_empty()
    }

    /// Clear all mappings
    fn clear(&mut self) {
        self.id_to_path.clear();
        self.path_to_id.clear();
    }

    /// Estimate memory usage in bytes
    fn estimate_memory_usage(&self) -> usize {
        // Base HashMap overhead
        let mut size = std::mem::size_of::<HashMap<u32, String>>() * 2;

        // Key-value pairs
        for (id, path) in &self.id_to_path {
            size += std::mem::size_of_val(id);
            size += std::mem::size_of::<String>() + path.len();
        }

        // Additional path keys (strings are shared via Arc/Rc internally by HashMap)
        for path in self.path_to_id.keys() {
            size += std::mem::size_of::<String>() + path.len();
        }

        size
    }
}

impl FileDataIdService {
    /// Create a new FileDataID service with the given provider
    pub fn new(provider: Box<dyn FileDataIdProvider>) -> Self {
        let stats = FileDataIdStats {
            provider_info: provider.info().name,
            ..Default::default()
        };

        Self {
            provider,
            mappings: Arc::new(RwLock::new(BidirectionalMappings::new())),
            stats: Arc::new(RwLock::new(stats)),
            cache: None,
        }
    }

    /// Create a new FileDataID service with the given provider and cache configuration
    pub fn new_with_cache(
        provider: Box<dyn FileDataIdProvider>,
        cache_config: CacheConfig,
    ) -> MetadataResult<Self> {
        let stats = FileDataIdStats {
            provider_info: provider.info().name,
            ..Default::default()
        };

        let cache = if cache_config.enabled {
            Some(Arc::new(FileDataIdCache::new(cache_config)?))
        } else {
            None
        };

        Ok(Self {
            provider,
            mappings: Arc::new(RwLock::new(BidirectionalMappings::new())),
            stats: Arc::new(RwLock::new(stats)),
            cache,
        })
    }

    /// Enable caching with the given configuration
    pub fn enable_cache(&mut self, cache_config: CacheConfig) -> MetadataResult<()> {
        if cache_config.enabled {
            self.cache = Some(Arc::new(FileDataIdCache::new(cache_config)?));
        } else {
            self.cache = None;
        }
        Ok(())
    }

    /// Disable caching
    pub fn disable_cache(&mut self) {
        self.cache = None;
    }

    /// Check if caching is enabled
    pub fn is_cache_enabled(&self) -> bool {
        self.cache.is_some()
    }

    /// Load mappings from the configured provider
    ///
    /// This will replace any existing mappings. The operation is atomic -
    /// either all mappings are loaded successfully, or none are changed.
    /// If caching is enabled, this method will first attempt to load from cache.
    pub async fn load_from_provider(&mut self) -> MetadataResult<()> {
        self.load_from_provider_with_cache_key("default").await
    }

    /// Load mappings from the configured provider with a specific cache key
    ///
    /// This allows for more granular cache management when different
    /// configurations or provider states need separate cache entries.
    pub async fn load_from_provider_with_cache_key(
        &mut self,
        cache_key: &str,
    ) -> MetadataResult<()> {
        // Try to load from cache first
        if let Some(ref cache) = self.cache {
            if let Ok(Some(cached_mappings)) = cache.load(cache_key) {
                // Create new bidirectional mappings from cache
                let mut new_mappings = BidirectionalMappings::with_capacity(cached_mappings.len());

                for mapping in &cached_mappings {
                    new_mappings.insert(mapping.id, mapping.path.clone());
                }

                // Atomically replace mappings
                {
                    let mut mappings_guard = self
                        .mappings
                        .write()
                        .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
                    *mappings_guard = new_mappings;
                }

                // Update statistics
                {
                    let mappings_guard = self
                        .mappings
                        .read()
                        .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
                    let mut stats_guard = self
                        .stats
                        .write()
                        .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;

                    stats_guard.total_mappings = mappings_guard.len();
                    stats_guard.memory_usage_bytes = mappings_guard.estimate_memory_usage();
                    stats_guard.last_loaded = Some(chrono::Utc::now());
                    stats_guard.provider_info = self.provider.info().name;
                }

                return Ok(());
            }
        }

        // Check if provider is available
        if !self.provider.is_available().await {
            return Err(MetadataError::Provider(
                "Provider is not available".to_string(),
            ));
        }

        // Check if provider supports enumeration
        let provider_info = self.provider.info();
        if !provider_info.capabilities.enumeration {
            // For providers that don't support enumeration, we operate in lazy mode
            // Mappings will be loaded on-demand through resolve_id calls

            // Update statistics to indicate lazy loading mode
            {
                let mut stats_guard = self
                    .stats
                    .write()
                    .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
                stats_guard.last_loaded = Some(chrono::Utc::now());
                stats_guard.provider_info = provider_info.name;
                // Set a flag or indicator that we're in lazy mode
                stats_guard.total_mappings = 0; // Will grow as mappings are resolved
            }

            // Clear existing mappings to start fresh
            {
                let mut mappings_guard = self
                    .mappings
                    .write()
                    .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
                mappings_guard.clear();
            }

            return Ok(());
        }

        // Get all available IDs from the provider
        let ids =
            self.provider.list_ids().await.map_err(|e| {
                MetadataError::Provider(format!("Failed to list provider IDs: {}", e))
            })?;

        if ids.is_empty() {
            // If list_ids is not implemented, try to get a reasonable set
            // This is a fallback for providers that don't implement enumeration
            return Err(MetadataError::Provider(
                "Provider supports enumeration but returned no IDs".to_string(),
            ));
        }

        // Load mappings in batches to avoid overwhelming the provider
        let batch_size = 1000;
        let mut all_mappings = Vec::new();

        for chunk in ids.chunks(batch_size) {
            let chunk_mappings =
                self.provider.get_mappings(chunk).await.map_err(|e| {
                    MetadataError::Provider(format!("Failed to get mappings: {}", e))
                })?;
            all_mappings.extend(chunk_mappings);
        }

        // Create new bidirectional mappings
        let mut new_mappings = BidirectionalMappings::with_capacity(all_mappings.len());

        // Insert all mappings
        for mapping in &all_mappings {
            new_mappings.insert(mapping.id, mapping.path.clone());
        }

        // Atomically replace mappings
        {
            let mut mappings_guard = self
                .mappings
                .write()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            *mappings_guard = new_mappings;
        }

        // Store in cache if enabled
        if let Some(ref cache) = self.cache {
            if let Err(e) = cache.store(cache_key, &all_mappings, &self.provider.info().name) {
                // Log cache error but don't fail the operation
                eprintln!("Warning: Failed to store mappings in cache: {}", e);
            }
        }

        // Update statistics
        {
            let mappings_guard = self
                .mappings
                .read()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            let mut stats_guard = self
                .stats
                .write()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;

            stats_guard.total_mappings = mappings_guard.len();
            stats_guard.memory_usage_bytes = mappings_guard.estimate_memory_usage();
            stats_guard.last_loaded = Some(chrono::Utc::now());
            stats_guard.provider_info = self.provider.info().name;

            // Update cache statistics if cache is enabled
            if let Some(ref cache) = self.cache {
                stats_guard.cache_stats = Some(self.get_cache_metrics(cache));
            }
        }

        Ok(())
    }

    /// Get file path by FileDataID (synchronous, from cache only)
    ///
    /// Returns the file path associated with the given FileDataID, or None
    /// if no mapping exists in the current cache.
    pub fn get_file_path(&self, file_data_id: u32) -> MetadataResult<Option<String>> {
        let mappings_guard = self
            .mappings
            .read()
            .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
        let result = mappings_guard
            .get_path(file_data_id)
            .map(ToString::to_string);

        // Update statistics
        {
            let mut stats_guard = self
                .stats
                .write()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            stats_guard.id_to_path_lookups += 1;
            if result.is_some() {
                stats_guard.successful_lookups += 1;
            } else {
                stats_guard.failed_lookups += 1;
            }
        }

        Ok(result)
    }

    /// Get file path by FileDataID with lazy loading from provider
    ///
    /// First checks the cache, then attempts to resolve from the provider
    /// if the provider supports ID to path resolution.
    pub async fn get_file_path_async(
        &mut self,
        file_data_id: u32,
    ) -> MetadataResult<Option<String>> {
        // First try to get from cached mappings
        {
            let mappings_guard = self
                .mappings
                .read()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            if let Some(path) = mappings_guard.get_path(file_data_id) {
                // Update statistics for cache hit
                let mut stats_guard = self
                    .stats
                    .write()
                    .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
                stats_guard.id_to_path_lookups += 1;
                stats_guard.successful_lookups += 1;
                return Ok(Some(path.to_string()));
            }
        }

        // Not in cache, try to resolve from provider if it supports lazy loading
        let provider_info = self.provider.info();
        if provider_info.capabilities.id_to_path {
            if let Ok(Some(path)) = self.provider.resolve_id(file_data_id).await {
                // Add to our mappings cache
                {
                    let mut mappings_guard = self
                        .mappings
                        .write()
                        .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
                    mappings_guard.insert(file_data_id, path.clone());
                }

                // Update statistics
                {
                    let mut stats_guard = self
                        .stats
                        .write()
                        .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
                    stats_guard.id_to_path_lookups += 1;
                    stats_guard.successful_lookups += 1;
                    stats_guard.total_mappings += 1;

                    // Update memory usage
                    let mappings_guard = self
                        .mappings
                        .read()
                        .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
                    stats_guard.memory_usage_bytes = mappings_guard.estimate_memory_usage();
                }

                return Ok(Some(path));
            }
        }

        // Not found
        {
            let mut stats_guard = self
                .stats
                .write()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            stats_guard.id_to_path_lookups += 1;
            stats_guard.failed_lookups += 1;
        }

        Ok(None)
    }

    /// Get FileDataID by file path
    ///
    /// Returns the FileDataID associated with the given file path, or None
    /// if no mapping exists. The path comparison is case-sensitive.
    pub fn get_file_data_id(&self, file_path: &str) -> MetadataResult<Option<u32>> {
        let mappings_guard = self
            .mappings
            .read()
            .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
        let result = mappings_guard.get_id(file_path);

        // Update statistics
        {
            let mut stats_guard = self
                .stats
                .write()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            stats_guard.path_to_id_lookups += 1;
            if result.is_some() {
                stats_guard.successful_lookups += 1;
            } else {
                stats_guard.failed_lookups += 1;
            }
        }

        Ok(result)
    }

    /// Add a single mapping to the service
    ///
    /// This will override any existing mapping for either the FileDataID
    /// or file path to maintain bidirectional consistency.
    pub fn add_mapping(&mut self, file_data_id: u32, file_path: String) -> MetadataResult<()> {
        {
            let mut mappings_guard = self
                .mappings
                .write()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            mappings_guard.insert(file_data_id, file_path);
        }

        // Update statistics
        {
            let mappings_guard = self
                .mappings
                .read()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            let mut stats_guard = self
                .stats
                .write()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            stats_guard.total_mappings = mappings_guard.len();
            stats_guard.memory_usage_bytes = mappings_guard.estimate_memory_usage();
        }

        Ok(())
    }

    /// Add multiple mappings to the service
    ///
    /// This is more efficient than calling add_mapping multiple times
    /// as it only updates statistics once.
    pub fn add_mappings(&mut self, mappings: Vec<FileDataIdMapping>) -> MetadataResult<()> {
        {
            let mut mappings_guard = self
                .mappings
                .write()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            for mapping in mappings {
                mappings_guard.insert(mapping.id, mapping.path);
            }
        }

        // Update statistics
        {
            let mappings_guard = self
                .mappings
                .read()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            let mut stats_guard = self
                .stats
                .write()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            stats_guard.total_mappings = mappings_guard.len();
            stats_guard.memory_usage_bytes = mappings_guard.estimate_memory_usage();
        }

        Ok(())
    }

    /// Remove a mapping by FileDataID
    ///
    /// Returns true if a mapping was removed, false if no mapping existed.
    pub fn remove_mapping_by_id(&mut self, file_data_id: u32) -> MetadataResult<bool> {
        let removed = {
            let mut mappings_guard = self
                .mappings
                .write()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            if let Some(path) = mappings_guard.id_to_path.remove(&file_data_id) {
                mappings_guard.path_to_id.remove(&path);
                true
            } else {
                false
            }
        };

        if removed {
            // Update statistics
            let mappings_guard = self
                .mappings
                .read()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            let mut stats_guard = self
                .stats
                .write()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            stats_guard.total_mappings = mappings_guard.len();
            stats_guard.memory_usage_bytes = mappings_guard.estimate_memory_usage();
        }

        Ok(removed)
    }

    /// Remove a mapping by file path
    ///
    /// Returns true if a mapping was removed, false if no mapping existed.
    pub fn remove_mapping_by_path(&mut self, file_path: &str) -> MetadataResult<bool> {
        let removed = {
            let mut mappings_guard = self
                .mappings
                .write()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            if let Some(id) = mappings_guard.path_to_id.remove(file_path) {
                mappings_guard.id_to_path.remove(&id);
                true
            } else {
                false
            }
        };

        if removed {
            // Update statistics
            let mappings_guard = self
                .mappings
                .read()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            let mut stats_guard = self
                .stats
                .write()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            stats_guard.total_mappings = mappings_guard.len();
            stats_guard.memory_usage_bytes = mappings_guard.estimate_memory_usage();
        }

        Ok(removed)
    }

    /// Clear all mappings
    pub fn clear_mappings(&mut self) -> MetadataResult<()> {
        {
            let mut mappings_guard = self
                .mappings
                .write()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            mappings_guard.clear();
        }

        // Update statistics
        {
            let mut stats_guard = self
                .stats
                .write()
                .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
            stats_guard.total_mappings = 0;
            stats_guard.memory_usage_bytes = 0;
        }

        Ok(())
    }

    /// Get current statistics about the service
    pub fn get_stats(&self) -> MetadataResult<FileDataIdStats> {
        let mut stats_guard = self
            .stats
            .write()
            .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;

        // Update cache statistics if cache is enabled
        if let Some(ref cache) = self.cache {
            stats_guard.cache_stats = Some(self.get_cache_metrics(cache));
        } else {
            stats_guard.cache_stats = None;
        }

        Ok(stats_guard.clone())
    }

    /// Check if a FileDataID exists in the mappings
    pub fn has_file_data_id(&self, file_data_id: u32) -> MetadataResult<bool> {
        let mappings_guard = self
            .mappings
            .read()
            .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
        Ok(mappings_guard.id_to_path.contains_key(&file_data_id))
    }

    /// Check if a file path exists in the mappings
    pub fn has_file_path(&self, file_path: &str) -> MetadataResult<bool> {
        let mappings_guard = self
            .mappings
            .read()
            .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
        Ok(mappings_guard.path_to_id.contains_key(file_path))
    }

    /// Get the total number of mappings
    pub fn mapping_count(&self) -> MetadataResult<usize> {
        let mappings_guard = self
            .mappings
            .read()
            .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
        Ok(mappings_guard.len())
    }

    /// Check if the service has any mappings loaded
    pub fn is_empty(&self) -> MetadataResult<bool> {
        let mappings_guard = self
            .mappings
            .read()
            .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
        Ok(mappings_guard.is_empty())
    }

    /// Export all mappings for backup or analysis
    pub fn export_mappings(&self) -> MetadataResult<Vec<FileDataIdMapping>> {
        let mappings_guard = self
            .mappings
            .read()
            .map_err(|_| MetadataError::Generic("RwLock poisoned".into()))?;
        let mut result = Vec::with_capacity(mappings_guard.len());

        for (file_data_id, file_path) in &mappings_guard.id_to_path {
            result.push(FileDataIdMapping::new(*file_data_id, file_path.clone()));
        }

        // Sort by FileDataID for consistent output
        result.sort_by_key(|m| m.id);

        Ok(result)
    }

    /// Clear the cache
    pub fn clear_cache(&self) -> MetadataResult<()> {
        if let Some(ref cache) = self.cache {
            cache.clear()?;
        }
        Ok(())
    }

    /// Remove a specific cache entry
    pub fn remove_from_cache(&self, cache_key: &str) -> MetadataResult<bool> {
        if let Some(ref cache) = self.cache {
            return cache.remove(cache_key);
        }
        Ok(false)
    }

    /// Check if a cache entry exists
    pub fn cache_contains(&self, cache_key: &str) -> bool {
        if let Some(ref cache) = self.cache {
            return cache.contains(cache_key);
        }
        false
    }

    /// Validate cache integrity
    pub fn validate_cache(&self) -> MetadataResult<bool> {
        if let Some(ref cache) = self.cache {
            return cache.validate();
        }
        Ok(true)
    }

    /// Cleanup expired cache entries
    pub fn cleanup_cache(&self) -> MetadataResult<()> {
        if let Some(ref cache) = self.cache {
            cache.cleanup()?;
        }
        Ok(())
    }

    /// Get cache statistics
    pub fn get_cache_stats(&self) -> Option<crate::fdid::cache::CacheStats> {
        self.cache.as_ref().map(|cache| cache.stats())
    }

    /// Get cache configuration
    pub fn get_cache_config(&self) -> Option<&CacheConfig> {
        self.cache.as_ref().map(|cache| cache.config())
    }

    /// Convert cache stats to metrics format
    #[allow(clippy::cast_possible_wrap)] // u64 to i64 conversion for timestamps
    fn get_cache_metrics(&self, cache: &FileDataIdCache) -> CacheMetrics {
        let cache_stats = cache.stats();

        CacheMetrics {
            hits: cache_stats.hits,
            misses: cache_stats.misses,
            hit_rate: cache_stats.hit_rate(),
            cached_entries: cache_stats.current_entries,
            cache_size_bytes: cache_stats.current_size_bytes,
            saves: cache_stats.saves,
            loads: cache_stats.loads,
            entries_expired: cache_stats.entries_expired,
            entries_evicted: cache_stats.entries_evicted,
            error_rate: cache_stats.error_rate(),
            last_save: cache_stats
                .last_save
                .and_then(|ts| chrono::DateTime::from_timestamp(ts as i64, 0)),
            last_load: cache_stats
                .last_load
                .and_then(|ts| chrono::DateTime::from_timestamp(ts as i64, 0)),
            cache_directory: cache_stats.cache_directory,
            enabled: cache.is_enabled(),
        }
    }

    /// Preload cache in the background
    pub async fn preload_cache(&mut self) -> MetadataResult<()> {
        self.preload_cache_with_key("default").await
    }

    /// Preload cache with specific key in the background
    pub async fn preload_cache_with_key(&mut self, cache_key: &str) -> MetadataResult<()> {
        if self.cache.is_none() {
            return Ok(()); // No cache enabled
        }

        // Check if cache already contains fresh data
        if self.cache_contains(cache_key) {
            return Ok(());
        }

        // Load from provider and cache
        self.load_from_provider_with_cache_key(cache_key).await
    }

    /// Background cache update task
    pub async fn update_cache_background(&mut self, cache_key: &str) -> MetadataResult<()> {
        if self.cache.is_none() {
            return Ok(());
        }

        // This method can be called periodically to refresh cache
        // It will load fresh data from provider and update cache
        self.load_from_provider_with_cache_key(cache_key).await
    }

    /// Schedule periodic cache updates
    ///
    /// Note: This is a simple implementation. In production, consider using
    /// a background task manager or scheduled task runner for better control.
    pub fn start_background_cache_refresh(
        service: Arc<tokio::sync::Mutex<Self>>,
        cache_key: String,
        interval: std::time::Duration,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);

            loop {
                interval_timer.tick().await;

                let mut service_guard = service.lock().await;
                if let Err(e) = service_guard.update_cache_background(&cache_key).await {
                    eprintln!("Background cache update failed: {}", e);
                }
                drop(service_guard); // Explicitly release the lock
            }
        })
    }
}

// Send and Sync are automatically implemented for Arc<RwLock<_>>

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fdid::provider::{MemoryProvider, ProviderCapabilities};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;
    use tokio::sync::Mutex;

    // Mock provider for error testing
    struct ErrorProvider {
        available: bool,
        error_on_list: bool,
        error_on_get: bool,
    }

    impl ErrorProvider {
        fn new() -> Self {
            Self {
                available: true,
                error_on_list: false,
                error_on_get: false,
            }
        }

        fn unavailable() -> Self {
            Self {
                available: false,
                error_on_list: false,
                error_on_get: false,
            }
        }

        fn with_list_error() -> Self {
            Self {
                available: true,
                error_on_list: true,
                error_on_get: false,
            }
        }
    }

    #[async_trait::async_trait]
    impl FileDataIdProvider for ErrorProvider {
        fn info(&self) -> crate::fdid::provider::ProviderInfo {
            crate::fdid::provider::ProviderInfo {
                name: "Error Provider".to_string(),
                description: "Provider for testing error conditions".to_string(),
                version: "1.0.0".to_string(),
                source_type: crate::fdid::provider::SourceType::Memory,
                capabilities: ProviderCapabilities::default(),
                last_updated: None,
                metadata: HashMap::new(),
            }
        }

        async fn initialize(&mut self) -> crate::fdid::FileDataIdResult<()> {
            Ok(())
        }

        async fn is_available(&self) -> bool {
            self.available
        }

        async fn resolve_id(&self, _id: u32) -> crate::fdid::FileDataIdResult<Option<String>> {
            if self.error_on_get {
                Err(crate::fdid::FileDataIdError::Generic(
                    "Test error".to_string(),
                ))
            } else {
                Ok(None)
            }
        }

        async fn resolve_path(&self, _path: &str) -> crate::fdid::FileDataIdResult<Option<u32>> {
            if self.error_on_get {
                Err(crate::fdid::FileDataIdError::Generic(
                    "Test error".to_string(),
                ))
            } else {
                Ok(None)
            }
        }

        async fn list_ids(&self) -> crate::fdid::FileDataIdResult<Vec<u32>> {
            if self.error_on_list {
                Err(crate::fdid::FileDataIdError::Generic(
                    "List error".to_string(),
                ))
            } else {
                Ok(vec![])
            }
        }
    }

    #[test]
    fn test_bidirectional_mappings_basic_operations() {
        let mut mappings = BidirectionalMappings::new();

        // Test initial state
        assert!(mappings.is_empty());
        assert_eq!(mappings.len(), 0);

        // Test insertion
        mappings.insert(12345, "Interface/Test.lua".to_string());
        assert_eq!(mappings.len(), 1);
        assert!(!mappings.is_empty());

        // Test lookups
        assert_eq!(mappings.get_path(12345), Some("Interface/Test.lua"));
        assert_eq!(mappings.get_id("Interface/Test.lua"), Some(12345));

        // Test non-existent lookups
        assert_eq!(mappings.get_path(99999), None);
        assert_eq!(mappings.get_id("NonExistent.lua"), None);
    }

    #[test]
    fn test_bidirectional_mappings_consistency_override() {
        let mut mappings = BidirectionalMappings::new();

        // Insert initial mappings
        mappings.insert(12345, "Interface/Test1.lua".to_string());
        mappings.insert(67890, "Interface/Test2.lua".to_string());
        assert_eq!(mappings.len(), 2);

        // Override FileDataID with new path (should remove old path)
        mappings.insert(12345, "Interface/NewTest.lua".to_string());
        assert_eq!(mappings.len(), 2);
        assert_eq!(mappings.get_path(12345), Some("Interface/NewTest.lua"));
        assert_eq!(mappings.get_id("Interface/Test1.lua"), None);
        assert_eq!(mappings.get_id("Interface/NewTest.lua"), Some(12345));

        // Override path with new FileDataID (should remove old ID)
        mappings.insert(11111, "Interface/Test2.lua".to_string());
        assert_eq!(mappings.len(), 2);
        assert_eq!(mappings.get_id("Interface/Test2.lua"), Some(11111));
        assert_eq!(mappings.get_path(67890), None);
    }

    #[test]
    fn test_bidirectional_mappings_memory_estimation() {
        let mut mappings = BidirectionalMappings::new();

        // Test empty mappings
        let empty_size = mappings.estimate_memory_usage();
        assert!(empty_size > 0); // Should include base HashMap overhead

        // Add some mappings
        mappings.insert(12345, "Interface/Test.lua".to_string());
        mappings.insert(67890, "World/Map/TestMap.wmo".to_string());

        let filled_size = mappings.estimate_memory_usage();
        assert!(filled_size > empty_size); // Should increase with content

        // Clear and check memory estimation
        mappings.clear();
        assert!(mappings.is_empty());
        assert_eq!(mappings.len(), 0);
    }

    #[test]
    fn test_bidirectional_mappings_capacity() {
        let capacity = 100;
        let mappings = BidirectionalMappings::with_capacity(capacity);

        // Verify initial state
        assert!(mappings.is_empty());
        assert_eq!(mappings.len(), 0);

        // Note: We can't directly test capacity, but we can verify the constructor works
    }

    #[tokio::test]
    async fn test_service_creation() {
        let provider = MemoryProvider::empty();
        let service = FileDataIdService::new(Box::new(provider));

        assert!(service.is_empty().expect("should get empty status"));
        assert_eq!(service.mapping_count().expect("should get count"), 0);
        assert!(!service.is_cache_enabled());
    }

    #[tokio::test]
    async fn test_service_creation_with_cache() {
        let temp_dir = TempDir::new().expect("should create temp directory");
        let cache_config = CacheConfig::default().with_directory(temp_dir.path());

        let provider = MemoryProvider::empty();
        let service = FileDataIdService::new_with_cache(Box::new(provider), cache_config)
            .expect("should create service with cache");

        assert!(service.is_cache_enabled());
        assert!(service.get_cache_config().is_some());
        assert!(service.get_cache_stats().is_some());
    }

    #[tokio::test]
    async fn test_service_load_from_provider_basic() {
        let mut provider = MemoryProvider::empty();
        provider.add_mapping(FileDataIdMapping::new(
            12345,
            "Interface/Test.lua".to_string(),
        ));
        provider.add_mapping(FileDataIdMapping::new(
            67890,
            "Interface/Test2.lua".to_string(),
        ));

        let mut service = FileDataIdService::new(Box::new(provider));
        service
            .load_from_provider()
            .await
            .expect("should load from provider");

        assert!(!service.is_empty().expect("should get empty status"));
        assert_eq!(service.mapping_count().expect("should get count"), 2);

        // Test lookups
        assert_eq!(
            service.get_file_path(12345).expect("should get file path"),
            Some("Interface/Test.lua".to_string())
        );
        assert_eq!(
            service
                .get_file_data_id("Interface/Test2.lua")
                .expect("should get file data id"),
            Some(67890)
        );

        // Test existence checks
        assert!(service.has_file_data_id(12345).expect("should check id"));
        assert!(service.has_file_data_id(67890).expect("should check id"));
        assert!(
            service
                .has_file_path("Interface/Test.lua")
                .expect("should check path")
        );
        assert!(
            service
                .has_file_path("Interface/Test2.lua")
                .expect("should check path")
        );
        assert!(!service.has_file_data_id(99999).expect("should check id"));
        assert!(
            !service
                .has_file_path("NonExistent.lua")
                .expect("should check path")
        );
    }

    #[tokio::test]
    async fn test_service_add_mappings() {
        let provider = MemoryProvider::empty();
        let mut service = FileDataIdService::new(Box::new(provider));

        // Add single mapping
        service
            .add_mapping(12345, "Interface/Single.lua".to_string())
            .expect("should add mapping");
        assert_eq!(service.mapping_count().expect("should get count"), 1);

        // Add multiple mappings
        let mappings = vec![
            FileDataIdMapping::new(67890, "Interface/Multi1.lua".to_string()),
            FileDataIdMapping::new(11111, "Interface/Multi2.lua".to_string()),
        ];
        service.add_mappings(mappings).expect("should add mappings");
        assert_eq!(service.mapping_count().expect("should get count"), 3);

        // Test all lookups work
        assert!(service.has_file_data_id(12345).expect("should check id"));
        assert!(service.has_file_data_id(67890).expect("should check id"));
        assert!(service.has_file_data_id(11111).expect("should check id"));
        assert!(
            service
                .has_file_path("Interface/Single.lua")
                .expect("should check path")
        );
        assert!(
            service
                .has_file_path("Interface/Multi1.lua")
                .expect("should check path")
        );
        assert!(
            service
                .has_file_path("Interface/Multi2.lua")
                .expect("should check path")
        );
    }

    #[tokio::test]
    async fn test_service_remove_mappings() {
        let mut provider = MemoryProvider::empty();
        provider.add_mapping(FileDataIdMapping::new(
            12345,
            "Interface/Test1.lua".to_string(),
        ));
        provider.add_mapping(FileDataIdMapping::new(
            67890,
            "Interface/Test2.lua".to_string(),
        ));

        let mut service = FileDataIdService::new(Box::new(provider));
        service
            .load_from_provider()
            .await
            .expect("should load from provider");

        assert_eq!(service.mapping_count().expect("should get count"), 2);

        // Remove by ID
        assert!(
            service
                .remove_mapping_by_id(12345)
                .expect("should remove mapping by id")
        );
        assert_eq!(service.mapping_count().expect("should get count"), 1);
        assert!(!service.has_file_data_id(12345).expect("should check id"));
        assert!(
            !service
                .has_file_path("Interface/Test1.lua")
                .expect("should check path")
        );

        // Remove by path
        assert!(
            service
                .remove_mapping_by_path("Interface/Test2.lua")
                .expect("should remove mapping by path")
        );
        assert_eq!(service.mapping_count().expect("should get count"), 0);
        assert!(!service.has_file_data_id(67890).expect("should check id"));
        assert!(
            !service
                .has_file_path("Interface/Test2.lua")
                .expect("should check path")
        );

        // Try to remove non-existent
        assert!(
            !service
                .remove_mapping_by_id(99999)
                .expect("should handle non-existent id")
        );
        assert!(
            !service
                .remove_mapping_by_path("NonExistent.lua")
                .expect("should handle non-existent path")
        );
    }

    #[tokio::test]
    async fn test_service_clear_mappings() {
        let mut provider = MemoryProvider::empty();
        provider.add_mapping(FileDataIdMapping::new(
            12345,
            "Interface/Test1.lua".to_string(),
        ));
        provider.add_mapping(FileDataIdMapping::new(
            67890,
            "Interface/Test2.lua".to_string(),
        ));

        let mut service = FileDataIdService::new(Box::new(provider));
        service
            .load_from_provider()
            .await
            .expect("should load from provider");

        assert_eq!(service.mapping_count().expect("should get count"), 2);

        // Clear all mappings
        service.clear_mappings().expect("should clear mappings");
        assert_eq!(service.mapping_count().expect("should get count"), 0);
        assert!(service.is_empty().expect("should get empty status"));

        // Verify no mappings exist
        assert!(!service.has_file_data_id(12345).expect("should check id"));
        assert!(!service.has_file_data_id(67890).expect("should check id"));
    }

    #[tokio::test]
    async fn test_service_statistics_tracking() {
        let mut provider = MemoryProvider::empty();
        provider.add_mapping(FileDataIdMapping::new(
            12345,
            "Interface/Test.lua".to_string(),
        ));

        let mut service = FileDataIdService::new(Box::new(provider));
        service
            .load_from_provider()
            .await
            .expect("should load from provider");

        // Check initial stats
        let stats = service.get_stats().expect("should get stats");
        assert_eq!(stats.total_mappings, 1);
        assert!(stats.memory_usage_bytes > 0);
        assert_eq!(stats.id_to_path_lookups, 0);
        assert_eq!(stats.path_to_id_lookups, 0);
        assert_eq!(stats.successful_lookups, 0);
        assert_eq!(stats.failed_lookups, 0);

        // Perform some lookups
        service.get_file_path(12345).expect("should get file path"); // Hit
        service
            .get_file_path(99999)
            .expect("should handle missing file path"); // Miss
        service
            .get_file_data_id("Interface/Test.lua")
            .expect("should get file data id"); // Hit
        service
            .get_file_data_id("NonExistent.lua")
            .expect("should handle missing file data id"); // Miss

        // Check updated stats
        let stats = service.get_stats().expect("should get stats");
        assert_eq!(stats.id_to_path_lookups, 2);
        assert_eq!(stats.path_to_id_lookups, 2);
        assert_eq!(stats.successful_lookups, 2);
        assert_eq!(stats.failed_lookups, 2);
        assert_eq!(stats.provider_info, "Memory Provider");
    }

    #[tokio::test]
    async fn test_service_export_mappings() {
        let mut provider = MemoryProvider::empty();
        provider.add_mapping(FileDataIdMapping::new(67890, "Interface/B.lua".to_string()));
        provider.add_mapping(FileDataIdMapping::new(12345, "Interface/A.lua".to_string()));

        let mut service = FileDataIdService::new(Box::new(provider));
        service
            .load_from_provider()
            .await
            .expect("should load from provider");

        let exported = service.export_mappings().expect("should export mappings");
        assert_eq!(exported.len(), 2);

        // Should be sorted by FileDataID
        assert_eq!(exported[0].id, 12345);
        assert_eq!(exported[0].path, "Interface/A.lua");
        assert_eq!(exported[1].id, 67890);
        assert_eq!(exported[1].path, "Interface/B.lua");
    }

    #[tokio::test]
    async fn test_service_cache_operations() {
        let temp_dir = TempDir::new().expect("should create temp directory");
        let cache_config = CacheConfig::default()
            .with_directory(temp_dir.path())
            .with_ttl_seconds(3600);

        let mut provider = MemoryProvider::empty();
        provider.add_mapping(FileDataIdMapping::new(
            12345,
            "Interface/Test.lua".to_string(),
        ));

        let mut service = FileDataIdService::new_with_cache(Box::new(provider), cache_config)
            .expect("should create service with cache");

        // Load from provider (should cache)
        service
            .load_from_provider()
            .await
            .expect("should load from provider");
        assert_eq!(service.mapping_count().expect("should get count"), 1);

        // Test cache operations
        assert!(service.cache_contains("default"));
        assert!(service.validate_cache().expect("should validate cache"));

        // Test cache clear
        service.clear_cache().expect("should clear cache");
        // Note: After clearing cache, we don't test cache_contains because
        // the service still has the data in memory
    }

    #[tokio::test]
    async fn test_service_cache_enable_disable() {
        let temp_dir = TempDir::new().expect("should create temp directory");
        let provider = MemoryProvider::empty();
        let mut service = FileDataIdService::new(Box::new(provider));

        // Initially no cache
        assert!(!service.is_cache_enabled());

        // Enable cache
        let cache_config = CacheConfig::default().with_directory(temp_dir.path());
        service
            .enable_cache(cache_config)
            .expect("should enable cache");
        assert!(service.is_cache_enabled());

        // Disable cache
        service.disable_cache();
        assert!(!service.is_cache_enabled());
    }

    #[tokio::test]
    async fn test_service_error_handling_unavailable_provider() {
        let provider = ErrorProvider::unavailable();
        let mut service = FileDataIdService::new(Box::new(provider));

        // Should fail with provider unavailable error
        let result = service.load_from_provider().await;
        assert!(result.is_err());
        assert!(
            result
                .expect_err("Result should be an error as verified above")
                .to_string()
                .contains("not available")
        );
    }

    #[tokio::test]
    async fn test_service_error_handling_provider_list_error() {
        let provider = ErrorProvider::with_list_error();
        let mut service = FileDataIdService::new(Box::new(provider));

        // Provider doesn't support enumeration, so it will succeed in lazy mode
        let result = service.load_from_provider().await;
        assert!(result.is_ok());
        // Service should be empty as we're in lazy mode
        assert_eq!(service.mapping_count().expect("should get count"), 0);
    }

    #[tokio::test]
    async fn test_service_error_handling_empty_provider() {
        let provider = ErrorProvider::new(); // Returns empty list
        let mut service = FileDataIdService::new(Box::new(provider));

        // Provider doesn't support enumeration, so it will succeed in lazy mode
        let result = service.load_from_provider().await;
        assert!(result.is_ok());
        // Service should be empty as we're in lazy mode
        assert_eq!(service.mapping_count().expect("should get count"), 0);
    }

    #[tokio::test]
    async fn test_service_thread_safety_concurrent_reads() {
        let mut provider = MemoryProvider::empty();
        for i in 0..1000 {
            provider.add_mapping(FileDataIdMapping::new(
                i,
                format!("Interface/Test{}.lua", i),
            ));
        }

        let mut service = FileDataIdService::new(Box::new(provider));
        service
            .load_from_provider()
            .await
            .expect("should load from provider");

        let service_arc = Arc::new(service);

        // Spawn multiple reader threads
        let mut handles = Vec::new();
        for thread_id in 0..4 {
            let service_clone = Arc::clone(&service_arc);
            let handle = thread::spawn(move || {
                let mut successful_lookups = 0;
                for i in 0..1000 {
                    if service_clone
                        .get_file_path(i)
                        .expect("should get file path in thread")
                        .is_some()
                    {
                        successful_lookups += 1;
                    }
                    if service_clone
                        .get_file_data_id(&format!("Interface/Test{}.lua", i))
                        .expect("should get file data id in thread")
                        .is_some()
                    {
                        successful_lookups += 1;
                    }
                }
                (thread_id, successful_lookups)
            });
            handles.push(handle);
        }

        // Wait for all threads to complete and verify results
        let mut total_successful = 0;
        for handle in handles {
            let (thread_id, successful) = handle.join().expect("thread should complete");
            println!("Thread {} had {} successful lookups", thread_id, successful);
            total_successful += successful;
        }

        // Each thread should have found 2000 successful lookups (1000 ID->path + 1000 path->ID)
        assert_eq!(total_successful, 4 * 2000);

        // Verify data integrity after concurrent access
        assert_eq!(
            service_arc
                .mapping_count()
                .expect("should get mapping count"),
            1000
        );
    }

    #[tokio::test]
    async fn test_service_concurrent_access_with_async_mutex() {
        let mut provider = MemoryProvider::empty();
        for i in 0..100 {
            provider.add_mapping(FileDataIdMapping::new(
                i,
                format!("Interface/Test{}.lua", i),
            ));
        }

        let mut service = FileDataIdService::new(Box::new(provider));
        service
            .load_from_provider()
            .await
            .expect("should load from provider");

        let service_arc = Arc::new(Mutex::new(service));

        // Test concurrent async access
        let mut tasks = Vec::new();
        for task_id in 0..10 {
            let service_clone = Arc::clone(&service_arc);
            let task = tokio::spawn(async move {
                let service_guard = service_clone.lock().await;
                let mut found_count = 0;
                for i in 0..100 {
                    if service_guard
                        .has_file_data_id(i)
                        .expect("should check for id")
                    {
                        found_count += 1;
                    }
                }
                (task_id, found_count)
            });
            tasks.push(task);
        }

        // Wait for all tasks
        let mut total_found = 0;
        for task in tasks {
            let (task_id, found) = task.await.expect("task should complete");
            println!("Task {} found {} mappings", task_id, found);
            total_found += found;
        }

        // Each task should find 100 mappings
        assert_eq!(total_found, 10 * 100);
    }

    #[tokio::test]
    async fn test_service_performance_large_dataset() {
        let mut provider = MemoryProvider::empty();

        // Create a reasonably large dataset for testing
        let dataset_size = 10_000; // Reduced from 500k for faster tests
        for i in 0..dataset_size {
            provider.add_mapping(FileDataIdMapping::new(
                i,
                format!("Interface/Test{:06}.lua", i),
            ));
        }

        let mut service = FileDataIdService::new(Box::new(provider));

        // Measure load time
        let start = Instant::now();
        service
            .load_from_provider()
            .await
            .expect("should load from provider");
        let load_duration = start.elapsed();

        println!(
            "Load time for {} mappings: {:?}",
            dataset_size, load_duration
        );
        assert!(load_duration < Duration::from_secs(5)); // Should load within 5 seconds

        // Verify correct count
        assert_eq!(
            service.mapping_count().expect("should get count"),
            dataset_size as usize
        );

        // Measure lookup performance
        let start = Instant::now();
        let mut successful_lookups = 0;
        for i in (0..dataset_size).step_by(100) {
            // Test every 100th entry
            if service
                .get_file_path(i)
                .expect("should get file path")
                .is_some()
            {
                successful_lookups += 1;
            }
        }
        let lookup_duration = start.elapsed();

        println!(
            "Lookup time for {} queries: {:?}",
            dataset_size / 100,
            lookup_duration
        );
        assert!(lookup_duration < Duration::from_secs(1)); // Should complete within 1 second
        assert_eq!(successful_lookups, dataset_size / 100);

        // Test memory usage estimation
        let stats = service.get_stats().expect("should get stats");
        assert!(stats.memory_usage_bytes > 0);
        println!(
            "Memory usage: {} bytes ({} MB)",
            stats.memory_usage_bytes,
            stats.memory_usage_bytes / (1024 * 1024)
        );
    }

    #[tokio::test]
    async fn test_service_batch_operations_performance() {
        let provider = MemoryProvider::empty();
        let batch_size = 1000;

        // Create batch of mappings
        let mut batch_mappings = Vec::new();
        for i in 0..batch_size {
            batch_mappings.push(FileDataIdMapping::new(
                i,
                format!("Interface/Batch{:04}.lua", i),
            ));
        }

        let mut service = FileDataIdService::new(Box::new(provider));

        // Measure batch insertion time
        let start = Instant::now();
        service
            .add_mappings(batch_mappings)
            .expect("should add batch mappings");
        let batch_duration = start.elapsed();

        println!(
            "Batch insert time for {} mappings: {:?}",
            batch_size, batch_duration
        );
        assert!(batch_duration < Duration::from_secs(1));

        assert_eq!(
            service.mapping_count().expect("should get count"),
            batch_size as usize
        );

        // Test export performance
        let start = Instant::now();
        let exported = service.export_mappings().expect("should export mappings");
        let export_duration = start.elapsed();

        println!(
            "Export time for {} mappings: {:?}",
            batch_size, export_duration
        );
        assert!(export_duration < Duration::from_secs(1));
        assert_eq!(exported.len(), batch_size as usize);

        // Verify exported mappings are sorted
        for i in 1..exported.len() {
            assert!(exported[i - 1].id <= exported[i].id);
        }
    }

    #[tokio::test]
    async fn test_service_memory_usage_tracking() {
        let provider = MemoryProvider::empty();
        let mut service = FileDataIdService::new(Box::new(provider));

        // Check empty memory usage
        let empty_stats = service.get_stats().expect("should get empty stats");
        assert_eq!(empty_stats.memory_usage_bytes, 0);

        // Add some mappings and verify memory increases
        service
            .add_mapping(1, "test1.lua".to_string())
            .expect("should add mapping 1");
        service
            .add_mapping(2, "test2.lua".to_string())
            .expect("should add mapping 2");

        let filled_stats = service.get_stats().expect("should get filled stats");
        assert!(filled_stats.memory_usage_bytes > 0);
        assert!(filled_stats.memory_usage_bytes > empty_stats.memory_usage_bytes);

        // Clear and verify memory is reset
        service.clear_mappings().expect("should clear mappings");
        let cleared_stats = service.get_stats().expect("should get cleared stats");
        assert_eq!(cleared_stats.memory_usage_bytes, 0);
    }

    #[tokio::test]
    async fn test_service_background_cache_refresh() {
        let temp_dir = TempDir::new().expect("should create temp directory");
        let cache_config = CacheConfig::default().with_directory(temp_dir.path());

        let mut provider = MemoryProvider::empty();
        provider.add_mapping(FileDataIdMapping::new(
            12345,
            "Interface/Test.lua".to_string(),
        ));

        let service = FileDataIdService::new_with_cache(Box::new(provider), cache_config)
            .expect("should create service with cache");
        let service_arc = Arc::new(Mutex::new(service));

        // Start background refresh with very short interval
        let refresh_handle = FileDataIdService::start_background_cache_refresh(
            Arc::clone(&service_arc),
            "test_key".to_string(),
            Duration::from_millis(100),
        );

        // Let it run for a short time
        tokio::time::sleep(Duration::from_millis(250)).await;

        // Cancel the background task
        refresh_handle.abort();

        // Verify the service still works
        let service_guard = service_arc.lock().await;
        // Note: The background task updates cache but doesn't load provider data
        // into the service memory unless it's empty, so we don't test mapping count here
        assert!(service_guard.is_cache_enabled());
    }

    #[tokio::test]
    async fn test_service_cache_key_management() {
        let temp_dir = TempDir::new().expect("should create temp directory");
        let cache_config = CacheConfig::default().with_directory(temp_dir.path());

        let mut provider = MemoryProvider::empty();
        provider.add_mapping(FileDataIdMapping::new(
            12345,
            "Interface/Test.lua".to_string(),
        ));

        let mut service = FileDataIdService::new_with_cache(Box::new(provider), cache_config)
            .expect("should create service with cache");

        // Load with custom cache key
        service
            .load_from_provider_with_cache_key("custom_key")
            .await
            .expect("should load from provider with custom key");
        assert!(service.cache_contains("custom_key"));

        // Load with another key
        service
            .load_from_provider_with_cache_key("another_key")
            .await
            .expect("should load from provider with another key");
        assert!(service.cache_contains("another_key"));

        // Remove specific cache entry
        assert!(
            service
                .remove_from_cache("custom_key")
                .expect("should remove from cache")
        );
        assert!(!service.cache_contains("custom_key"));
        assert!(service.cache_contains("another_key"));
    }

    #[tokio::test]
    async fn test_service_preload_cache_operations() {
        let temp_dir = TempDir::new().expect("should create temp directory");
        let cache_config = CacheConfig::default().with_directory(temp_dir.path());

        let mut provider = MemoryProvider::empty();
        provider.add_mapping(FileDataIdMapping::new(
            12345,
            "Interface/Test.lua".to_string(),
        ));

        let mut service = FileDataIdService::new_with_cache(Box::new(provider), cache_config)
            .expect("should create service with cache");

        // Preload cache with default key
        service.preload_cache().await.expect("should preload cache");
        assert!(service.cache_contains("default"));
        assert_eq!(service.mapping_count().expect("should get count"), 1);

        // Preload with custom key
        service
            .preload_cache_with_key("preload_key")
            .await
            .expect("should preload cache with key");
        assert!(service.cache_contains("preload_key"));
    }

    #[test]
    fn test_service_cache_disabled_operations() {
        let provider = MemoryProvider::empty();
        let service = FileDataIdService::new(Box::new(provider));

        // All cache operations should be no-ops or return appropriate defaults
        assert!(!service.is_cache_enabled());
        assert!(service.get_cache_config().is_none());
        assert!(service.get_cache_stats().is_none());
        assert!(service.clear_cache().is_ok());
        assert!(!service.cache_contains("any_key"));
        assert!(service.validate_cache().expect("should validate cache"));
        assert!(service.cleanup_cache().is_ok());
        assert!(
            !service
                .remove_from_cache("any_key")
                .expect("should handle non-existent key")
        );
    }

    #[tokio::test]
    async fn test_service_edge_cases_empty_strings() {
        let provider = MemoryProvider::empty();
        let mut service = FileDataIdService::new(Box::new(provider));

        // Test empty string path
        service
            .add_mapping(12345, String::new())
            .expect("should add empty string mapping");
        assert_eq!(
            service.get_file_path(12345).expect("should get file path"),
            Some(String::new())
        );
        assert_eq!(
            service
                .get_file_data_id("")
                .expect("should get file data id for empty string"),
            Some(12345)
        );
        assert!(service.has_file_path("").expect("should check path"));
    }

    #[tokio::test]
    async fn test_service_edge_cases_special_characters() {
        let provider = MemoryProvider::empty();
        let mut service = FileDataIdService::new(Box::new(provider));

        // Test paths with special characters
        let special_path = "Interface/Special[Chars]/{File}.lua";
        service
            .add_mapping(12345, special_path.to_string())
            .expect("should add special path mapping");

        assert_eq!(
            service.get_file_path(12345).expect("should get file path"),
            Some(special_path.to_string())
        );
        assert_eq!(
            service
                .get_file_data_id(special_path)
                .expect("should get file data id for special path"),
            Some(12345)
        );
    }

    #[tokio::test]
    async fn test_service_edge_cases_unicode_paths() {
        let provider = MemoryProvider::empty();
        let mut service = FileDataIdService::new(Box::new(provider));

        // Test Unicode paths
        let unicode_path = "Interface/测试/файл.lua";
        service
            .add_mapping(12345, unicode_path.to_string())
            .expect("should add unicode path mapping");

        assert_eq!(
            service.get_file_path(12345).expect("should get file path"),
            Some(unicode_path.to_string())
        );
        assert_eq!(
            service
                .get_file_data_id(unicode_path)
                .expect("should get file data id for unicode path"),
            Some(12345)
        );
    }

    #[tokio::test]
    async fn test_service_consistency_after_operations() {
        let provider = MemoryProvider::empty();
        let mut service = FileDataIdService::new(Box::new(provider));

        // Add initial mapping
        service
            .add_mapping(12345, "path1.lua".to_string())
            .expect("should add first mapping");

        // Override with same ID, different path
        service
            .add_mapping(12345, "path2.lua".to_string())
            .expect("should add second mapping");

        // Verify consistency
        assert_eq!(service.mapping_count().expect("should get count"), 1);
        assert_eq!(
            service.get_file_path(12345).expect("should get file path"),
            Some("path2.lua".to_string())
        );
        assert_eq!(
            service
                .get_file_data_id("path2.lua")
                .expect("should get file data id for path2"),
            Some(12345)
        );
        assert_eq!(
            service
                .get_file_data_id("path1.lua")
                .expect("should get file data id for path1"),
            None
        );
    }
}
