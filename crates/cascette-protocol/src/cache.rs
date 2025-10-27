//! High-performance cache wrapper for protocol operations
//!
//! This module provides optimized caching for NGDP/CASC protocol operations
//! by integrating with the cascette-cache multi-layer caching system.
//! This implementation provides a protocol-friendly interface over the
//! comprehensive caching infrastructure.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use bytes::Bytes;
use cascette_cache::{
    config::{DiskCacheConfig, MemoryCacheConfig},
    disk_cache::DiskCache,
    memory_cache::MemoryCache,
    traits::AsyncCache,
};
use tokio::runtime::Handle;

use crate::config::CacheConfig;
use crate::error::Result;

/// Simple string-based cache key compatible with cascette-cache
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolCacheKey {
    key: String,
    cached_key: OnceLock<String>,
}

impl std::hash::Hash for ProtocolCacheKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}

impl cascette_cache::key::CacheKey for ProtocolCacheKey {
    fn as_cache_key(&self) -> &str {
        self.cached_key.get_or_init(|| self.key.clone())
    }
}

impl ProtocolCacheKey {
    pub fn new(key: String) -> Self {
        Self {
            key,
            cached_key: OnceLock::new(),
        }
    }

    pub fn ribbit(endpoint: &str) -> Self {
        Self::new(format!("ribbit:{}", endpoint))
    }

    pub fn cdn(content_type: &str, hash: &str) -> Self {
        Self::new(format!("cdn:{}:{}", content_type, hash))
    }

    pub fn config(config_type: &str, hash: &str) -> Self {
        Self::new(format!("config:{}:{}", config_type, hash))
    }
}

/// High-performance protocol cache backed by cascette-cache
///
/// This implementation integrates with the cascette-cache system,
/// using either memory or disk cache based on configuration.
pub struct ProtocolCache {
    cache: Arc<dyn AsyncCache<ProtocolCacheKey> + Send + Sync>,
    config: CacheConfig,
}

impl ProtocolCache {
    /// Create a new high-performance protocol cache
    pub fn new(config: &CacheConfig) -> Result<Self> {
        // Use disk cache if cache_dir is provided, otherwise use memory cache
        let cache: Arc<dyn AsyncCache<ProtocolCacheKey> + Send + Sync> =
            if let Some(ref cache_dir) = config.cache_dir {
                // Create disk cache without subdirectories to preserve CDN path structure
                let disk_config = DiskCacheConfig::new(cache_dir)
                    .with_max_disk_usage(config.disk_max_size_bytes)
                    .with_max_files(100_000) // Allow up to 100k cached files
                    .with_default_ttl(config.cdn_ttl)
                    .with_subdirectories(false, 0); // Disable subdirectories to preserve CDN paths

                let disk_cache = DiskCache::new(disk_config).map_err(|e| {
                    crate::error::ProtocolError::Cache(CacheError::Backend(e.to_string()))
                })?;

                Arc::new(disk_cache)
            } else {
                // Fall back to memory cache if no cache directory is specified
                let memory_config = MemoryCacheConfig::new()
                    .with_max_entries(config.memory_max_items)
                    .with_max_memory(config.memory_max_size_bytes)
                    .with_default_ttl(config.ribbit_ttl);

                let memory_cache = MemoryCache::new(memory_config).map_err(|e| {
                    crate::error::ProtocolError::Cache(CacheError::Backend(e.to_string()))
                })?;

                Arc::new(memory_cache)
            };

        Ok(Self {
            cache,
            config: config.clone(),
        })
    }

    /// Execute an async operation with proper runtime handling
    fn execute_async<F, T>(&self, operation: F) -> Result<T>
    where
        F: std::future::Future<Output = cascette_cache::error::CacheResult<T>> + Send + 'static,
        T: Send + 'static,
    {
        // Try to use current runtime first, fallback to new runtime
        match Handle::try_current() {
            Ok(_) => {
                // We're in a tokio context, but can't block_on
                // Use spawn_blocking to run in a separate thread with its own runtime
                let result = std::thread::spawn(move || {
                    let rt = match tokio::runtime::Runtime::new() {
                        Ok(rt) => rt,
                        Err(e) => {
                            return Err(cascette_cache::CacheError::Backend(format!(
                                "Failed to create tokio runtime: {}",
                                e
                            )));
                        }
                    };
                    rt.block_on(operation)
                })
                .join()
                .map_err(|_| {
                    crate::error::ProtocolError::Cache(CacheError::Runtime(
                        "Failed to execute async operation in thread".to_string(),
                    ))
                })?;
                result.map_err(|e| {
                    crate::error::ProtocolError::Cache(CacheError::Backend(e.to_string()))
                })
            }
            Err(_) => {
                // No runtime available, create one
                let rt = tokio::runtime::Runtime::new().map_err(|e| {
                    crate::error::ProtocolError::Cache(CacheError::Runtime(format!(
                        "Failed to create runtime: {}",
                        e
                    )))
                })?;
                rt.block_on(operation).map_err(|e| {
                    crate::error::ProtocolError::Cache(CacheError::Backend(e.to_string()))
                })
            }
        }
    }

    /// Get data from cache with legacy string key support
    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let cache_key = self.parse_legacy_key(key);
        let cache = self.cache.clone();

        let result = self.execute_async(async move { cache.get(&cache_key).await })?;

        Ok(result.map(|bytes| bytes.to_vec()))
    }

    /// Get bytes from cache - alias for `get()` for backward compatibility
    pub fn get_bytes(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.get(key)
    }

    /// Parse legacy string-based cache keys into typed keys
    fn parse_legacy_key(&self, key: &str) -> ProtocolCacheKey {
        ProtocolCacheKey::new(key.to_string())
    }

    /// Determine TTL based on cache key type
    fn get_ttl_for_key(&self, key: &ProtocolCacheKey) -> Duration {
        if key.key.starts_with("ribbit:") {
            self.config.ribbit_ttl
        } else if key.key.starts_with("cdn:") {
            self.config.cdn_ttl
        } else {
            self.config.config_ttl
        }
    }

    /// Store data with TTL using optimized operations
    pub fn store_with_ttl(&self, key: &str, data: &[u8], ttl: Duration) -> Result<()> {
        let cache_key = self.parse_legacy_key(key);
        let bytes = Bytes::copy_from_slice(data);
        let cache = self.cache.clone();

        self.execute_async(async move { cache.put_with_ttl(cache_key, bytes, ttl).await })
    }

    /// Store bytes without TTL (uses protocol-specific TTL)
    pub fn store_bytes(&self, key: &str, data: &[u8]) -> Result<()> {
        let cache_key = self.parse_legacy_key(key);
        let ttl = self.get_ttl_for_key(&cache_key);
        self.store_with_ttl(key, data, ttl)
    }

    /// Clean up expired entries - delegated to cascette-cache
    pub fn cleanup_expired(&self) -> Result<usize> {
        // cascette-cache handles cleanup automatically
        // Return 0 to maintain API compatibility
        Ok(0)
    }

    /// Get cache statistics for performance monitoring
    pub fn stats(&self) -> Result<CacheStats> {
        let cache = self.cache.clone();
        let stats = self.execute_async(async move { cache.stats().await })?;

        Ok(CacheStats {
            hits: stats.hit_count,
            misses: stats.miss_count,
            entries: stats.entry_count as u64,
            memory_usage: stats.memory_usage_bytes as u64,
            disk_usage: 0, // Memory cache only
        })
    }

    /// Pre-warm cache with expected keys to improve cold start performance
    #[allow(clippy::unused_async)] // Future enhancement hook
    pub async fn warm_cache(&self, keys: Vec<String>) -> Result<usize> {
        let mut warmed = 0;
        for key in keys {
            let cache_key = self.parse_legacy_key(&key);
            if matches!(self.cache.contains(&cache_key).await, Ok(true)) {
                warmed += 1;
            }
        }
        Ok(warmed)
    }

    /// Get cache hit rate as percentage
    pub fn hit_rate(&self) -> Result<f64> {
        let stats = self.stats()?;
        Ok(stats.hit_rate())
    }

    /// Clear the entire cache
    pub fn clear(&self) -> Result<()> {
        let cache = self.cache.clone();
        self.execute_async(async move { cache.clear().await })
    }

    /// Get current cache size
    pub fn len(&self) -> Result<usize> {
        let cache = self.cache.clone();
        self.execute_async(async move { cache.size().await })
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> Result<bool> {
        let cache = self.cache.clone();
        self.execute_async(async move { cache.is_empty().await })
    }
}

/// Cache statistics for monitoring
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub entries: u64,
    pub memory_usage: u64,
    pub disk_usage: u64,
}

impl CacheStats {
    /// Calculate cache hit rate as percentage
    pub fn hit_rate(&self) -> f64 {
        if self.hits + self.misses == 0 {
            0.0
        } else {
            (self.hits as f64) / ((self.hits + self.misses) as f64) * 100.0
        }
    }

    /// Get total cache usage in bytes
    pub fn total_usage(&self) -> u64 {
        self.memory_usage + self.disk_usage
    }
}

/// Enhanced error type for cache operations
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Backend cache error: {0}")]
    Backend(String),

    #[error("Runtime error: {0}")]
    Runtime(String),

    #[error("Cache error: {0}")]
    Other(String),
}

// Note: From<CacheError> is already derived in error.rs via #[from]
