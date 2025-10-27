//! Persistent caching system for FileDataID mappings
//!
//! This module provides a production-ready caching layer for FileDataID mappings
//! that supports persistent storage, cache validation, configurable policies,
//! and comprehensive metrics collection.

use crate::error::{MetadataError, MetadataResult};
use crate::fdid::provider::FileDataIdMapping;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt, fs,
    path::{Path, PathBuf},
    sync::{
        Arc, RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

/// Cache configuration for FileDataID mappings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Enable persistent cache
    pub enabled: bool,
    /// Cache directory path
    pub directory: Option<PathBuf>,
    /// Default TTL for cache entries in seconds
    pub default_ttl_seconds: u64,
    /// Maximum cache size in MB (0 = unlimited)
    pub max_size_mb: u64,
    /// Maximum number of entries (0 = unlimited)
    pub max_entries: usize,
    /// Cache file compression
    pub compress: bool,
    /// Background cleanup interval in seconds
    pub cleanup_interval_seconds: u64,
    /// Enable atomic operations for corruption resistance
    pub atomic_writes: bool,
    /// Cache validation frequency in seconds
    pub validation_interval_seconds: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            directory: None,
            default_ttl_seconds: 24 * 60 * 60, // 24 hours
            max_size_mb: 100,                  // 100 MB
            max_entries: 500_000,              // 500k mappings
            compress: true,
            cleanup_interval_seconds: 60 * 60, // 1 hour
            atomic_writes: true,
            validation_interval_seconds: 60 * 60, // 1 hour
        }
    }
}

impl CacheConfig {
    /// Create a new cache configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable cache with specific directory
    #[must_use]
    pub fn with_directory<P: Into<PathBuf>>(mut self, directory: P) -> Self {
        self.directory = Some(directory.into());
        self
    }

    /// Set TTL in seconds
    #[must_use]
    pub fn with_ttl_seconds(mut self, ttl_seconds: u64) -> Self {
        self.default_ttl_seconds = ttl_seconds;
        self
    }

    /// Set maximum cache size in MB
    #[must_use]
    pub fn with_max_size_mb(mut self, max_size_mb: u64) -> Self {
        self.max_size_mb = max_size_mb;
        self
    }

    /// Set maximum number of entries
    #[must_use]
    pub fn with_max_entries(mut self, max_entries: usize) -> Self {
        self.max_entries = max_entries;
        self
    }

    /// Enable or disable compression
    #[must_use]
    pub fn with_compression(mut self, compress: bool) -> Self {
        self.compress = compress;
        self
    }

    /// Disable cache
    #[must_use]
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Validate configuration
    pub fn validate(&self) -> MetadataResult<()> {
        if self.enabled {
            if self.default_ttl_seconds == 0 {
                return Err(MetadataError::InvalidConfiguration(
                    "TTL must be greater than 0".to_string(),
                ));
            }
            if self.cleanup_interval_seconds == 0 {
                return Err(MetadataError::InvalidConfiguration(
                    "Cleanup interval must be greater than 0".to_string(),
                ));
            }
            if self.validation_interval_seconds == 0 {
                return Err(MetadataError::InvalidConfiguration(
                    "Validation interval must be greater than 0".to_string(),
                ));
            }
        }
        Ok(())
    }
}

/// Cache statistics with detailed metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CacheStats {
    /// Cache hit count
    pub hits: u64,
    /// Cache miss count
    pub misses: u64,
    /// Number of entries stored
    pub entries_stored: u64,
    /// Number of entries evicted
    pub entries_evicted: u64,
    /// Number of entries expired
    pub entries_expired: u64,
    /// Current number of entries
    pub current_entries: usize,
    /// Current cache size in bytes
    pub current_size_bytes: u64,
    /// Last successful load timestamp
    pub last_load: Option<u64>,
    /// Last successful save timestamp
    pub last_save: Option<u64>,
    /// Load operation count
    pub loads: u64,
    /// Save operation count
    pub saves: u64,
    /// Load errors
    pub load_errors: u64,
    /// Save errors
    pub save_errors: u64,
    /// Validation checks performed
    pub validations: u64,
    /// Corruption incidents detected
    pub corruption_detected: u64,
    /// Cache directory path
    pub cache_directory: String,
}

impl CacheStats {
    /// Calculate hit rate as percentage
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            (self.hits as f64 / total as f64) * 100.0
        }
    }

    /// Calculate error rate as percentage
    pub fn error_rate(&self) -> f64 {
        let total_ops = self.loads + self.saves;
        let errors = self.load_errors + self.save_errors;
        if total_ops == 0 {
            0.0
        } else {
            (errors as f64 / total_ops as f64) * 100.0
        }
    }
}

/// Cache entry metadata
#[derive(Debug, Clone, Serialize, Deserialize, bincode::Encode, bincode::Decode)]
struct CacheEntryMeta {
    /// Creation timestamp (seconds since UNIX epoch)
    created_at: u64,
    /// TTL in seconds
    ttl_seconds: u64,
    /// Entry size in bytes
    size_bytes: u64,
    /// Version for cache validation
    version: u32,
    /// Checksum for corruption detection
    checksum: u64,
}

impl CacheEntryMeta {
    /// Create new cache entry metadata
    fn new(ttl_seconds: u64, size_bytes: u64) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            created_at: now,
            ttl_seconds,
            size_bytes,
            version: 1,
            checksum: 0, // Will be set after serialization
        }
    }

    /// Check if entry is expired
    fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        now > self.created_at + self.ttl_seconds
    }

    /// Calculate checksum for data
    fn calculate_checksum(data: &[u8]) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        hasher.finish()
    }

    /// Validate checksum
    fn validate_checksum(&self, data: &[u8]) -> bool {
        Self::calculate_checksum(data) == self.checksum
    }
}

/// Cached mappings with metadata
#[derive(Debug, Clone, Serialize, Deserialize, bincode::Encode, bincode::Decode)]
struct CachedMappings {
    /// Metadata about the cache entry
    meta: CacheEntryMeta,
    /// The actual FileDataID mappings
    mappings: Vec<FileDataIdMapping>,
    /// Provider information
    provider_info: String,
}

/// Atomic metrics for thread-safe statistics
#[derive(Debug)]
struct AtomicCacheMetrics {
    hits: AtomicU64,
    misses: AtomicU64,
    entries_stored: AtomicU64,
    entries_evicted: AtomicU64,
    entries_expired: AtomicU64,
    loads: AtomicU64,
    saves: AtomicU64,
    load_errors: AtomicU64,
    save_errors: AtomicU64,
    validations: AtomicU64,
    corruption_detected: AtomicU64,
}

impl Default for AtomicCacheMetrics {
    fn default() -> Self {
        Self {
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            entries_stored: AtomicU64::new(0),
            entries_evicted: AtomicU64::new(0),
            entries_expired: AtomicU64::new(0),
            loads: AtomicU64::new(0),
            saves: AtomicU64::new(0),
            load_errors: AtomicU64::new(0),
            save_errors: AtomicU64::new(0),
            validations: AtomicU64::new(0),
            corruption_detected: AtomicU64::new(0),
        }
    }
}

impl AtomicCacheMetrics {
    /// Convert to snapshot for reporting
    fn snapshot(
        &self,
        current_entries: usize,
        current_size_bytes: u64,
        cache_dir: &str,
    ) -> CacheStats {
        CacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            entries_stored: self.entries_stored.load(Ordering::Relaxed),
            entries_evicted: self.entries_evicted.load(Ordering::Relaxed),
            entries_expired: self.entries_expired.load(Ordering::Relaxed),
            current_entries,
            current_size_bytes,
            last_load: None, // Will be set by cache implementation
            last_save: None, // Will be set by cache implementation
            loads: self.loads.load(Ordering::Relaxed),
            saves: self.saves.load(Ordering::Relaxed),
            load_errors: self.load_errors.load(Ordering::Relaxed),
            save_errors: self.save_errors.load(Ordering::Relaxed),
            validations: self.validations.load(Ordering::Relaxed),
            corruption_detected: self.corruption_detected.load(Ordering::Relaxed),
            cache_directory: cache_dir.to_string(),
        }
    }
}

/// Persistent cache for FileDataID mappings
pub struct FileDataIdCache {
    /// Cache configuration
    config: CacheConfig,
    /// Cache directory path
    cache_dir: PathBuf,
    /// In-memory cache for fast access
    memory_cache: Arc<RwLock<HashMap<String, CachedMappings>>>,
    /// Atomic metrics
    metrics: Arc<AtomicCacheMetrics>,
    /// Last load timestamp
    last_load: Arc<RwLock<Option<u64>>>,
    /// Last save timestamp
    last_save: Arc<RwLock<Option<u64>>>,
}

impl fmt::Debug for FileDataIdCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileDataIdCache")
            .field("config", &self.config)
            .field("cache_dir", &self.cache_dir)
            .field(
                "memory_entries",
                &self
                    .memory_cache
                    .read()
                    .map(|guard| guard.len())
                    .unwrap_or(0),
            )
            .field("metrics", &"AtomicCacheMetrics")
            .field("last_load", &"Arc<RwLock<Option<u64>>>")
            .field("last_save", &"Arc<RwLock<Option<u64>>>")
            .finish()
    }
}

impl FileDataIdCache {
    /// Create a new persistent cache
    pub fn new(config: CacheConfig) -> MetadataResult<Self> {
        config.validate()?;

        if !config.enabled {
            return Ok(Self {
                config,
                cache_dir: PathBuf::new(),
                memory_cache: Arc::new(RwLock::new(HashMap::new())),
                metrics: Arc::new(AtomicCacheMetrics::default()),
                last_load: Arc::new(RwLock::new(None)),
                last_save: Arc::new(RwLock::new(None)),
            });
        }

        // Determine cache directory
        let cache_dir = if let Some(ref dir) = config.directory {
            dir.clone()
        } else {
            dirs::cache_dir()
                .ok_or_else(|| {
                    MetadataError::CacheError("Cannot determine cache directory".to_string())
                })?
                .join("cascette")
                .join("fdid")
        };

        // Create cache directory
        fs::create_dir_all(&cache_dir).map_err(|e| {
            MetadataError::CacheError(format!("Failed to create cache directory: {}", e))
        })?;

        Ok(Self {
            config,
            cache_dir,
            memory_cache: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(AtomicCacheMetrics::default()),
            last_load: Arc::new(RwLock::new(None)),
            last_save: Arc::new(RwLock::new(None)),
        })
    }

    /// Get cache file path for a key
    fn get_cache_path(&self, key: &str) -> PathBuf {
        let filename = key
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>();

        let extension = if self.config.compress {
            "cache.gz"
        } else {
            "cache"
        };
        self.cache_dir.join(format!("{}.{}", filename, extension))
    }

    /// Store mappings in cache
    pub fn store(
        &self,
        key: &str,
        mappings: &[FileDataIdMapping],
        provider_info: &str,
    ) -> MetadataResult<()> {
        if !self.config.enabled {
            return Ok(());
        }

        // Serialize mappings
        let cached = CachedMappings {
            meta: CacheEntryMeta::new(
                self.config.default_ttl_seconds,
                std::mem::size_of_val(mappings) as u64,
            ),
            mappings: mappings.to_vec(),
            provider_info: provider_info.to_string(),
        };

        let mut serialized =
            bincode::encode_to_vec(&cached, bincode::config::standard()).map_err(|e| {
                MetadataError::CacheError(format!("Failed to serialize cache data: {}", e))
            })?;

        // Update checksum
        let mut cached_with_checksum = cached;
        cached_with_checksum.meta.checksum = CacheEntryMeta::calculate_checksum(&serialized);

        serialized = bincode::encode_to_vec(&cached_with_checksum, bincode::config::standard())
            .map_err(|e| {
                MetadataError::CacheError(format!(
                    "Failed to serialize cache data with checksum: {}",
                    e
                ))
            })?;

        // Compress if enabled
        if self.config.compress {
            serialized = self.compress_data(&serialized)?;
        }

        // Write to disk
        let cache_path = self.get_cache_path(key);

        if self.config.atomic_writes {
            self.atomic_write(&cache_path, &serialized)?;
        } else {
            fs::write(&cache_path, &serialized).map_err(|e| {
                MetadataError::CacheError(format!("Failed to write cache file: {}", e))
            })?;
        }

        // Store in memory cache
        {
            let mut memory = self
                .memory_cache
                .write()
                .map_err(|_| MetadataError::CacheError("Memory cache lock poisoned".to_string()))?;
            memory.insert(key.to_string(), cached_with_checksum);

            // Enforce memory limits
            self.enforce_memory_limits(&mut memory);
        }

        // Update metrics
        self.metrics.entries_stored.fetch_add(1, Ordering::Relaxed);
        self.metrics.saves.fetch_add(1, Ordering::Relaxed);

        {
            let mut last_save = self
                .last_save
                .write()
                .map_err(|_| MetadataError::CacheError("Last save lock poisoned".to_string()))?;
            *last_save = Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            );
        }

        Ok(())
    }

    /// Load mappings from cache
    pub fn load(&self, key: &str) -> MetadataResult<Option<Vec<FileDataIdMapping>>> {
        if !self.config.enabled {
            return Ok(None);
        }

        // Check memory cache first
        {
            let memory = self
                .memory_cache
                .read()
                .map_err(|_| MetadataError::CacheError("Memory cache lock poisoned".to_string()))?;

            if let Some(cached) = memory.get(key) {
                if !cached.meta.is_expired() {
                    self.metrics.hits.fetch_add(1, Ordering::Relaxed);
                    return Ok(Some(cached.mappings.clone()));
                }
            }
        }

        // Load from disk
        let cache_path = self.get_cache_path(key);
        if !cache_path.exists() {
            self.metrics.misses.fetch_add(1, Ordering::Relaxed);
            return Ok(None);
        }

        let mut data = fs::read(&cache_path).map_err(|e| {
            self.metrics.load_errors.fetch_add(1, Ordering::Relaxed);
            MetadataError::CacheError(format!("Failed to read cache file: {}", e))
        })?;

        // Decompress if needed
        if self.config.compress {
            data = self.decompress_data(&data)?;
        }

        // Deserialize
        let cached: CachedMappings = bincode::decode_from_slice(&data, bincode::config::standard())
            .map_err(|e| {
                self.metrics.load_errors.fetch_add(1, Ordering::Relaxed);
                self.metrics
                    .corruption_detected
                    .fetch_add(1, Ordering::Relaxed);
                MetadataError::CacheError(format!("Failed to deserialize cache data: {}", e))
            })?
            .0;

        // Validate checksum
        if !cached.meta.validate_checksum(&data) {
            self.metrics
                .corruption_detected
                .fetch_add(1, Ordering::Relaxed);
            return Err(MetadataError::CacheError(
                "Cache data corruption detected".to_string(),
            ));
        }

        // Check if expired
        if cached.meta.is_expired() {
            self.metrics.entries_expired.fetch_add(1, Ordering::Relaxed);
            self.metrics.misses.fetch_add(1, Ordering::Relaxed);

            // Clean up expired file
            let _ = fs::remove_file(&cache_path);
            return Ok(None);
        }

        // Store in memory cache for faster access
        {
            let mut memory = self
                .memory_cache
                .write()
                .map_err(|_| MetadataError::CacheError("Memory cache lock poisoned".to_string()))?;
            memory.insert(key.to_string(), cached.clone());
        }

        // Update metrics
        self.metrics.hits.fetch_add(1, Ordering::Relaxed);
        self.metrics.loads.fetch_add(1, Ordering::Relaxed);

        {
            let mut last_load = self
                .last_load
                .write()
                .map_err(|_| MetadataError::CacheError("Last load lock poisoned".to_string()))?;
            *last_load = Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            );
        }

        Ok(Some(cached.mappings))
    }

    /// Check if key exists and is not expired
    pub fn contains(&self, key: &str) -> bool {
        if !self.config.enabled {
            return false;
        }

        // Check memory cache
        if let Ok(memory) = self.memory_cache.read() {
            if let Some(cached) = memory.get(key) {
                return !cached.meta.is_expired();
            }
        }

        // Check disk cache
        let cache_path = self.get_cache_path(key);
        cache_path.exists()
    }

    /// Remove entry from cache
    pub fn remove(&self, key: &str) -> MetadataResult<bool> {
        if !self.config.enabled {
            return Ok(false);
        }

        // Remove from memory
        let memory_removed = {
            let mut memory = self
                .memory_cache
                .write()
                .map_err(|_| MetadataError::CacheError("Memory cache lock poisoned".to_string()))?;
            memory.remove(key).is_some()
        };

        // Remove from disk
        let cache_path = self.get_cache_path(key);
        let disk_removed = if cache_path.exists() {
            fs::remove_file(&cache_path).map_err(|e| {
                MetadataError::CacheError(format!("Failed to remove cache file: {}", e))
            })?;
            true
        } else {
            false
        };

        let removed = memory_removed || disk_removed;

        if removed {
            self.metrics.entries_evicted.fetch_add(1, Ordering::Relaxed);
        }

        Ok(removed)
    }

    /// Clear all cache entries
    pub fn clear(&self) -> MetadataResult<()> {
        if !self.config.enabled {
            return Ok(());
        }

        // Clear memory cache
        {
            let mut memory = self
                .memory_cache
                .write()
                .map_err(|_| MetadataError::CacheError("Memory cache lock poisoned".to_string()))?;
            memory.clear();
        }

        // Clear disk cache
        if self.cache_dir.exists() {
            for entry in fs::read_dir(&self.cache_dir).map_err(|e| {
                MetadataError::CacheError(format!("Failed to read cache directory: {}", e))
            })? {
                let entry = entry.map_err(|e| {
                    MetadataError::CacheError(format!("Failed to read directory entry: {}", e))
                })?;

                if entry
                    .file_type()
                    .map_err(|e| {
                        MetadataError::CacheError(format!("Failed to get file type: {}", e))
                    })?
                    .is_file()
                {
                    let path = entry.path();
                    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                        if ext == "cache" || ext == "gz" {
                            fs::remove_file(&path).map_err(|e| {
                                MetadataError::CacheError(format!(
                                    "Failed to remove cache file: {}",
                                    e
                                ))
                            })?;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        let Ok(memory) = self.memory_cache.read() else {
            // If lock is poisoned, create empty stats
            return CacheStats {
                cache_directory: self.cache_dir.to_str().unwrap_or("").to_string(),
                ..Default::default()
            };
        };

        let current_entries = memory.len();
        let current_size_bytes = memory.values().map(|cached| cached.meta.size_bytes).sum();

        let mut stats = self.metrics.snapshot(
            current_entries,
            current_size_bytes,
            self.cache_dir.to_str().unwrap_or(""),
        );

        // Add timestamp information
        if let Ok(last_load) = self.last_load.read() {
            stats.last_load = *last_load;
        }

        if let Ok(last_save) = self.last_save.read() {
            stats.last_save = *last_save;
        }

        stats
    }

    /// Validate cache integrity
    pub fn validate(&self) -> MetadataResult<bool> {
        if !self.config.enabled {
            return Ok(true);
        }

        let mut valid = true;
        self.metrics.validations.fetch_add(1, Ordering::Relaxed);

        // Check memory cache consistency
        {
            let memory = self
                .memory_cache
                .read()
                .map_err(|_| MetadataError::CacheError("Memory cache lock poisoned".to_string()))?;

            for (key, cached) in memory.iter() {
                if cached.meta.is_expired() {
                    continue;
                }

                // Check if corresponding disk file exists
                let cache_path = self.get_cache_path(key);
                if !cache_path.exists() {
                    valid = false;
                    continue;
                }

                // Validate disk file by attempting to load it
                if self.load(key).is_err() {
                    valid = false;
                }
            }
        }

        Ok(valid)
    }

    /// Cleanup expired entries
    pub fn cleanup(&self) -> MetadataResult<()> {
        if !self.config.enabled {
            return Ok(());
        }

        let mut expired_keys = Vec::new();

        // Find expired entries in memory
        {
            let memory = self
                .memory_cache
                .read()
                .map_err(|_| MetadataError::CacheError("Memory cache lock poisoned".to_string()))?;

            for (key, cached) in memory.iter() {
                if cached.meta.is_expired() {
                    expired_keys.push(key.clone());
                }
            }
        }

        // Remove expired entries
        for key in expired_keys {
            self.remove(&key)?;
            self.metrics.entries_expired.fetch_add(1, Ordering::Relaxed);
        }

        // Cleanup orphaned disk files
        if self.cache_dir.exists() {
            for entry in fs::read_dir(&self.cache_dir).map_err(|e| {
                MetadataError::CacheError(format!("Failed to read cache directory: {}", e))
            })? {
                let entry = entry.map_err(|e| {
                    MetadataError::CacheError(format!("Failed to read directory entry: {}", e))
                })?;

                if entry
                    .file_type()
                    .map_err(|e| {
                        MetadataError::CacheError(format!("Failed to get file type: {}", e))
                    })?
                    .is_file()
                {
                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                        if name.ends_with(".cache") || name.ends_with(".cache.gz") {
                            // Try to determine key from filename
                            let key = if name.ends_with(".cache.gz") {
                                name.trim_end_matches(".cache.gz")
                            } else {
                                name.trim_end_matches(".cache")
                            };

                            // Check if this file represents an expired entry
                            if let Ok(Some(mappings)) = self.load(key) {
                                if mappings.is_empty() {
                                    let _ = fs::remove_file(&path);
                                }
                            } else {
                                // File is corrupted or expired, remove it
                                let _ = fs::remove_file(&path);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Get cache configuration
    pub fn config(&self) -> &CacheConfig {
        &self.config
    }

    /// Check if cache is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    // Private helper methods

    /// Compress data using gzip
    fn compress_data(&self, data: &[u8]) -> MetadataResult<Vec<u8>> {
        use std::io::prelude::*;

        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(data)
            .map_err(|e| MetadataError::CacheError(format!("Failed to compress data: {}", e)))?;

        encoder
            .finish()
            .map_err(|e| MetadataError::CacheError(format!("Failed to finish compression: {}", e)))
    }

    /// Decompress gzip data
    fn decompress_data(&self, data: &[u8]) -> MetadataResult<Vec<u8>> {
        use std::io::prelude::*;

        let mut decoder = flate2::read::GzDecoder::new(data);
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .map_err(|e| MetadataError::CacheError(format!("Failed to decompress data: {}", e)))?;

        Ok(decompressed)
    }

    /// Atomic write to prevent corruption
    fn atomic_write(&self, path: &Path, data: &[u8]) -> MetadataResult<()> {
        let temp_path = path.with_extension("tmp");

        fs::write(&temp_path, data).map_err(|e| {
            MetadataError::CacheError(format!("Failed to write temporary file: {}", e))
        })?;

        fs::rename(&temp_path, path).map_err(|e| {
            MetadataError::CacheError(format!("Failed to rename temporary file: {}", e))
        })?;

        Ok(())
    }

    /// Enforce memory cache limits
    fn enforce_memory_limits(&self, memory: &mut HashMap<String, CachedMappings>) {
        // Check entry count limit
        if self.config.max_entries > 0 && memory.len() > self.config.max_entries {
            let excess = memory.len() - self.config.max_entries;
            let mut entries_to_remove: Vec<_> = memory
                .iter()
                .map(|(key, cached)| (key.clone(), cached.meta.created_at))
                .collect();

            // Sort by creation time (oldest first)
            entries_to_remove.sort_by_key(|(_, created_at)| *created_at);

            for (key, _) in entries_to_remove.into_iter().take(excess) {
                memory.remove(&key);
                self.metrics.entries_evicted.fetch_add(1, Ordering::Relaxed);
            }
        }

        // Check size limit
        if self.config.max_size_mb > 0 {
            let max_size_bytes = self.config.max_size_mb * 1024 * 1024;
            let current_size: u64 = memory.values().map(|cached| cached.meta.size_bytes).sum();

            if current_size > max_size_bytes {
                let mut entries_to_remove: Vec<_> = memory
                    .iter()
                    .map(|(key, cached)| {
                        (key.clone(), cached.meta.created_at, cached.meta.size_bytes)
                    })
                    .collect();

                // Sort by creation time (oldest first)
                entries_to_remove.sort_by_key(|(_, created_at, _)| *created_at);

                let mut removed_size = 0u64;
                let target_size = current_size - max_size_bytes;

                for (key, _, size) in entries_to_remove {
                    memory.remove(&key);
                    removed_size += size;
                    self.metrics.entries_evicted.fetch_add(1, Ordering::Relaxed);

                    if removed_size >= target_size {
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::field_reassign_with_default,
    clippy::float_cmp,
    clippy::redundant_clone
)] // Acceptable in tests
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cache_config_default() {
        let config = CacheConfig::default();
        assert!(config.enabled);
        assert_eq!(config.default_ttl_seconds, 24 * 60 * 60);
        assert!(config.compress);
        assert!(config.atomic_writes);
    }

    #[test]
    fn test_cache_config_builder() {
        let config = CacheConfig::new()
            .with_directory("/tmp/test")
            .with_ttl_seconds(3600)
            .with_max_size_mb(50)
            .with_compression(false);

        assert_eq!(config.directory, Some(PathBuf::from("/tmp/test")));
        assert_eq!(config.default_ttl_seconds, 3600);
        assert_eq!(config.max_size_mb, 50);
        assert!(!config.compress);
    }

    #[test]
    fn test_cache_config_validation() {
        let mut config = CacheConfig::default();
        config.default_ttl_seconds = 0;
        assert!(config.validate().is_err());

        config.default_ttl_seconds = 3600;
        config.cleanup_interval_seconds = 0;
        assert!(config.validate().is_err());

        config.cleanup_interval_seconds = 60;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_cache_stats_calculations() {
        let mut stats = CacheStats::default();
        stats.hits = 80;
        stats.misses = 20;
        assert_eq!(stats.hit_rate(), 80.0);

        stats.loads = 100;
        stats.saves = 50;
        stats.load_errors = 5;
        stats.save_errors = 5;
        assert_eq!(stats.error_rate(), 100.0 * 10.0 / 150.0);
    }

    #[test]
    fn test_cache_entry_meta() {
        let meta = CacheEntryMeta::new(3600, 1024);
        assert_eq!(meta.ttl_seconds, 3600);
        assert_eq!(meta.size_bytes, 1024);
        assert_eq!(meta.version, 1);

        // Test expiration
        assert!(!meta.is_expired());

        let mut expired_meta = meta.clone();
        expired_meta.created_at = 0; // Very old timestamp
        assert!(expired_meta.is_expired());
    }

    #[test]
    fn test_cache_creation_disabled() {
        let config = CacheConfig::default().disabled();
        let cache = FileDataIdCache::new(config).expect("Test assertion");
        assert!(!cache.is_enabled());
    }

    #[test]
    fn test_cache_creation_enabled() {
        let temp_dir = TempDir::new().expect("Test assertion");
        let config = CacheConfig::default().with_directory(temp_dir.path());

        let cache = FileDataIdCache::new(config).expect("Test assertion");
        assert!(cache.is_enabled());
        assert!(cache.cache_dir.exists());
    }

    #[test]
    fn test_cache_store_and_load() {
        let temp_dir = TempDir::new().expect("Test assertion");
        let config = CacheConfig::default()
            .with_directory(temp_dir.path())
            .with_compression(false);

        let cache = FileDataIdCache::new(config).expect("Test assertion");

        let mappings = vec![
            FileDataIdMapping::new(12345, "Interface/Test1.lua".to_string()),
            FileDataIdMapping::new(67890, "Interface/Test2.lua".to_string()),
        ];

        // Store mappings
        cache
            .store("test-key", &mappings, "TestProvider")
            .expect("Test assertion");

        // Load mappings
        let loaded = cache.load("test-key").expect("Test assertion");
        assert!(loaded.is_some());
        assert_eq!(loaded.expect("Test assertion").len(), 2);

        // Check stats
        let stats = cache.stats();
        assert_eq!(stats.saves, 1);
        assert_eq!(stats.hits, 1);
    }

    #[test]
    fn test_cache_contains() {
        let temp_dir = TempDir::new().expect("Test assertion");
        let config = CacheConfig::default().with_directory(temp_dir.path());

        let cache = FileDataIdCache::new(config).expect("Test assertion");

        assert!(!cache.contains("nonexistent"));

        let mappings = vec![FileDataIdMapping::new(
            12345,
            "Interface/Test.lua".to_string(),
        )];

        cache
            .store("test-key", &mappings, "TestProvider")
            .expect("Test assertion");
        assert!(cache.contains("test-key"));
    }

    #[test]
    fn test_cache_remove() {
        let temp_dir = TempDir::new().expect("Test assertion");
        let config = CacheConfig::default().with_directory(temp_dir.path());

        let cache = FileDataIdCache::new(config).expect("Test assertion");

        let mappings = vec![FileDataIdMapping::new(
            12345,
            "Interface/Test.lua".to_string(),
        )];

        cache
            .store("test-key", &mappings, "TestProvider")
            .expect("Test assertion");
        assert!(cache.contains("test-key"));

        let removed = cache.remove("test-key").expect("Test assertion");
        assert!(removed);
        assert!(!cache.contains("test-key"));
    }

    #[test]
    fn test_cache_clear() {
        let temp_dir = TempDir::new().expect("Test assertion");
        let config = CacheConfig::default().with_directory(temp_dir.path());

        let cache = FileDataIdCache::new(config).expect("Test assertion");

        let mappings = vec![FileDataIdMapping::new(
            12345,
            "Interface/Test.lua".to_string(),
        )];

        cache
            .store("key1", &mappings, "TestProvider")
            .expect("Test assertion");
        cache
            .store("key2", &mappings, "TestProvider")
            .expect("Test assertion");

        assert!(cache.contains("key1"));
        assert!(cache.contains("key2"));

        cache.clear().expect("Test assertion");

        assert!(!cache.contains("key1"));
        assert!(!cache.contains("key2"));
    }

    #[test]
    fn test_disabled_cache_operations() {
        let config = CacheConfig::default().disabled();
        let cache = FileDataIdCache::new(config).expect("Test assertion");

        let mappings = vec![FileDataIdMapping::new(
            12345,
            "Interface/Test.lua".to_string(),
        )];

        // All operations should succeed but do nothing
        cache
            .store("key", &mappings, "TestProvider")
            .expect("Test assertion");
        assert_eq!(cache.load("key").expect("Test assertion"), None);
        assert!(!cache.contains("key"));
        assert!(!cache.remove("key").expect("Test assertion"));
        cache.clear().expect("Test assertion");
        cache.cleanup().expect("Test assertion");
        assert!(cache.validate().expect("Test assertion"));
    }
}
