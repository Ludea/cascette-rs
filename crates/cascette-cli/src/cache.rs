//! Cache implementation for cascette CLI
//!
//! Provides disk-based caching for protocol responses, product catalogs,
//! and imported data using simple file-based storage.

use anyhow::{Context, Result};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::{Duration, SystemTime},
};

/// Cache entry with metadata
#[derive(Debug, Clone)]
struct CacheEntry {
    data: Vec<u8>,
    created_at: SystemTime,
    ttl: Duration,
}

impl CacheEntry {
    fn is_expired(&self) -> bool {
        match self.created_at.elapsed() {
            Ok(elapsed) => elapsed > self.ttl,
            Err(_) => true, // If time went backwards, consider expired
        }
    }
}

/// Simple CLI cache implementation
pub struct CliCache {
    /// Cache directory
    cache_dir: PathBuf,
    /// In-memory cache for hot data
    memory_cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    /// Default TTL from config (used by store() method)
    #[allow(dead_code)] // Used by store() method which will be called when cache is integrated
    default_ttl: Duration,
    /// Maximum cache size in bytes (for future size-based eviction)
    #[allow(dead_code)] // Reserved for future size-based eviction feature
    max_size_bytes: u64,
}

impl CliCache {
    /// Create a new CLI cache instance
    pub fn new(config: &crate::config::CacheConfig) -> Result<Self> {
        // Get cache directory
        let cache_dir = if let Some(ref dir) = config.directory {
            if dir.is_absolute() {
                dir.clone()
            } else {
                crate::paths::cache_dir()?.join(dir)
            }
        } else {
            crate::paths::cache_dir()?
        };

        // Ensure cache directory exists
        fs::create_dir_all(&cache_dir)
            .with_context(|| format!("Failed to create cache directory: {:?}", cache_dir))?;

        Ok(Self {
            cache_dir,
            memory_cache: Arc::new(RwLock::new(HashMap::new())),
            default_ttl: Duration::from_secs(config.api_ttl_seconds),
            max_size_bytes: config.max_size_mb * 1024 * 1024,
        })
    }

    /// Get the cache file path for a key
    fn get_cache_path(&self, key: &str) -> PathBuf {
        // Determine the subdirectory based on the key prefix
        let (subdir, filename) = if key.starts_with("ribbit:") || key.starts_with("tact:") {
            // API responses go in the api folder with protocol prefix
            let safe_key = key
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>();
            ("api", safe_key)
        } else if key.starts_with("config:") {
            // Configuration files go in the config folder
            let safe_key = key
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>();
            ("config", safe_key)
        } else {
            // Other cache types in misc folder
            let safe_key = key
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>();
            ("misc", safe_key)
        };

        self.cache_dir
            .join(subdir)
            .join(format!("{}.cache", filename))
    }

    /// Store data with custom TTL
    pub fn store_with_ttl(&self, key: &str, data: &[u8], ttl_seconds: u64) -> Result<()> {
        let ttl = Duration::from_secs(ttl_seconds);

        // Store in memory cache
        {
            let mut cache = self.memory_cache.write().expect("Cache lock poisoned");
            cache.insert(
                key.to_string(),
                CacheEntry {
                    data: data.to_vec(),
                    created_at: SystemTime::now(),
                    ttl,
                },
            );

            // Simple memory limit: if we have too many entries, clear old ones
            if cache.len() > 1000 {
                cache.retain(|_, entry| !entry.is_expired());
            }
        }

        // Also store to disk
        let cache_path = self.get_cache_path(key);
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Write data with metadata
        let metadata = CacheMetadata {
            created_at: SystemTime::now(),
            ttl_seconds,
        };

        let serialized = bincode::encode_to_vec(&(metadata, data), bincode::config::standard())
            .context("Failed to serialize cache data")?;

        fs::write(&cache_path, serialized)
            .with_context(|| format!("Failed to write cache file: {:?}", cache_path))?;

        Ok(())
    }

    /// Retrieve data from cache
    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        // Check memory cache first
        {
            let mut cache = self.memory_cache.write().expect("Cache lock poisoned");
            if let Some(entry) = cache.get(key) {
                if !entry.is_expired() {
                    return Ok(Some(entry.data.clone()));
                } else {
                    // Remove expired entry from memory
                    cache.remove(key);
                }
            }
        }

        // Check disk cache
        let cache_path = self.get_cache_path(key);
        if !cache_path.exists() {
            return Ok(None);
        }

        let serialized = fs::read(&cache_path)
            .with_context(|| format!("Failed to read cache file: {:?}", cache_path))?;

        let (metadata, data): (CacheMetadata, Vec<u8>) =
            bincode::decode_from_slice(&serialized, bincode::config::standard())
                .context("Failed to deserialize cache data")?
                .0;

        // Check if expired
        if let Ok(elapsed) = metadata.created_at.elapsed() {
            if elapsed > Duration::from_secs(metadata.ttl_seconds) {
                // Remove expired file
                let _ = fs::remove_file(&cache_path);
                return Ok(None);
            }
        }

        // Store in memory cache for faster access next time
        {
            let mut cache = self.memory_cache.write().expect("Cache lock poisoned");
            cache.insert(
                key.to_string(),
                CacheEntry {
                    data: data.clone(),
                    created_at: metadata.created_at,
                    ttl: Duration::from_secs(metadata.ttl_seconds),
                },
            );
        }

        Ok(Some(data))
    }

    /// Clear all cached data
    pub fn clear_all(&self) -> Result<()> {
        // Clear memory cache
        {
            let mut cache = self.memory_cache.write().expect("Cache lock poisoned");
            cache.clear();
        }

        // Clear disk cache - remove all subdirectories and cache files
        if self.cache_dir.exists() {
            for entry in walkdir::WalkDir::new(&self.cache_dir)
                .min_depth(1)
                .max_depth(1)
                .into_iter()
                .filter_map(std::result::Result::ok)
            {
                let path = entry.path();
                if path.is_dir() {
                    fs::remove_dir_all(path)?;
                } else if path.extension().and_then(|s| s.to_str()) == Some("cache") {
                    fs::remove_file(path)?;
                }
            }
        }

        Ok(())
    }

    /// Clear cached data by pattern
    pub fn clear_pattern(&self, pattern: &str) -> Result<()> {
        if pattern == "*" || pattern == "all" {
            return self.clear_all();
        }

        // Clear from memory cache
        {
            let mut cache = self.memory_cache.write().expect("Cache lock poisoned");
            let prefix = pattern.trim_end_matches('*');
            cache.retain(|key, _| !key.starts_with(prefix));
        }

        // Clear from disk cache (simplified - just clear all for now)
        // A more sophisticated implementation would scan files and match patterns
        println!("Note: Pattern-based disk cache clearing not fully implemented");

        Ok(())
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        // Clean up expired entries before calculating stats
        {
            let mut cache = self.memory_cache.write().expect("Cache lock poisoned");
            cache.retain(|_, entry| !entry.is_expired());
        }

        let memory_cache = self.memory_cache.read().expect("Cache lock poisoned");
        let memory_entries = memory_cache.len();
        let memory_bytes: usize = memory_cache.values().map(|entry| entry.data.len()).sum();

        // Count disk cache files and size
        let mut disk_entries = 0;
        let mut disk_bytes = 0u64;

        if self.cache_dir.exists() {
            for entry in walkdir::WalkDir::new(&self.cache_dir)
                .max_depth(3)
                .into_iter()
                .filter_map(std::result::Result::ok)
            {
                if entry.file_type().is_file()
                    && entry.path().extension().and_then(|s| s.to_str()) == Some("cache")
                {
                    disk_entries += 1;
                    if let Ok(metadata) = entry.metadata() {
                        disk_bytes += metadata.len();
                    }
                }
            }
        }

        CacheStats {
            memory_entries,
            memory_bytes,
            disk_entries,
            disk_bytes: disk_bytes as usize,
        }
    }

    /// Get cache directory path
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }
}

/// Cache statistics
#[derive(Debug)]
pub struct CacheStats {
    pub memory_entries: usize,
    pub memory_bytes: usize,
    pub disk_entries: usize,
    pub disk_bytes: usize,
}

/// Cache metadata stored with each entry
#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
struct CacheMetadata {
    created_at: SystemTime,
    ttl_seconds: u64,
}

/// Global cache instance
static CACHE: std::sync::OnceLock<Arc<CliCache>> = std::sync::OnceLock::new();

/// Initialize the global cache
pub fn init_cache(config: &crate::config::CacheConfig) -> Result<()> {
    if !config.enabled {
        return Ok(());
    }

    let cache = CliCache::new(config)?;
    CACHE
        .set(Arc::new(cache))
        .map_err(|_| anyhow::anyhow!("Cache already initialized"))?;
    Ok(())
}

/// Get the global cache instance
pub fn get_cache() -> Option<&'static Arc<CliCache>> {
    CACHE.get()
}
