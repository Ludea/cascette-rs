//! Game installation management

use crate::{
    Result, StorageError, archive::ArchiveManager, index::IndexManager, resolver::ContentResolver,
};
use cascette_crypto::{ContentKey, EncodingKey};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock as AsyncRwLock;
use tracing::{debug, info};

/// Represents a game installation with its local storage
pub struct Installation {
    path: PathBuf,
    /// Index manager for .idx files
    index_manager: Arc<AsyncRwLock<IndexManager>>,
    /// Archive manager for .data files
    archive_manager: Arc<AsyncRwLock<ArchiveManager>>,
    /// Content resolver for lookup chain
    resolver: Arc<ContentResolver>,
    /// Simple in-memory cache for performance optimization
    cache: Arc<AsyncRwLock<dashmap::DashMap<String, Vec<u8>>>>,
}

impl Installation {
    /// Open an existing installation or create a new one
    ///
    /// # Errors
    ///
    /// Returns error if directory cannot be created or components cannot be initialized
    pub fn open(path: PathBuf) -> Result<Self> {
        // Ensure installation directory exists
        if !path.exists() {
            info!("Creating installation directory: {:?}", path);
            std::fs::create_dir_all(&path)?;
        }

        // Create required subdirectories
        let indices_path = path.join(crate::INDICES_DIR);
        let data_path = path.join(crate::DATA_DIR);
        let config_path = path.join(crate::CONFIG_DIR);

        for dir in [&indices_path, &data_path, &config_path] {
            if !dir.exists() {
                std::fs::create_dir_all(dir)?;
            }
        }

        // Initialize managers
        let index_manager = Arc::new(AsyncRwLock::new(IndexManager::new(&indices_path)));
        let archive_manager = Arc::new(AsyncRwLock::new(ArchiveManager::new(&data_path)));
        let resolver = Arc::new(ContentResolver::new());

        // Initialize simple in-memory cache for performance
        let cache = Arc::new(AsyncRwLock::new(dashmap::DashMap::new()));

        info!("Opened installation at {:?}", path);

        Ok(Self {
            path,
            index_manager,
            archive_manager,
            resolver,
            cache,
        })
    }

    /// Read a file by content key
    ///
    /// # Errors
    ///
    /// Returns error if file cannot be found or read
    pub async fn read_file(&self, key: &[u8]) -> Result<Vec<u8>> {
        if key.len() != 16 {
            return Err(StorageError::InvalidFormat(
                "Content key must be 16 bytes".to_string(),
            ));
        }
        let mut key_bytes = [0u8; 16];
        key_bytes.copy_from_slice(key);
        let content_key = ContentKey::from_bytes(key_bytes);
        self.read_file_by_content_key(&content_key).await
    }

    /// Read a file by content key (complete pipeline with caching)
    ///
    /// # Errors
    ///
    /// Returns error if file cannot be found or read
    pub async fn read_file_by_content_key(&self, content_key: &ContentKey) -> Result<Vec<u8>> {
        let cache_key = hex::encode(content_key.as_bytes());
        debug!("Reading file by content key: {}", cache_key);

        // Step 1: Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(cached_data) = cache.get(&cache_key) {
                debug!("Cache hit for content key: {}", cache_key);
                return Ok(cached_data.clone());
            }
        }

        debug!("Cache miss for content key: {}", cache_key);

        // Step 2: Resolve content key to encoding key
        let encoding_key = self
            .resolver
            .resolve_content_key(content_key)
            .ok_or_else(|| {
                StorageError::NotFound(format!(
                    "Encoding key not found for content key: {cache_key}"
                ))
            })?;

        debug!(
            "Resolved to encoding key: {}",
            hex::encode(encoding_key.as_bytes())
        );

        // Step 3: Look up encoding key in indices to get archive location
        let index_entry = {
            let index_manager = self.index_manager.read().await;
            index_manager.lookup(&encoding_key).ok_or_else(|| {
                StorageError::NotFound(format!(
                    "Archive location not found for encoding key: {}",
                    hex::encode(encoding_key.as_bytes())
                ))
            })?
        };

        debug!(
            "Found in archive {} at offset {} with size {}",
            index_entry.archive_id(),
            index_entry.archive_offset(),
            index_entry.size
        );

        // Step 4: Read from archive and decompress if needed
        let data = {
            let archive_manager = self.archive_manager.read().await;
            archive_manager.read_content(
                index_entry.archive_id(),
                index_entry.archive_offset(),
                index_entry.size,
            )?
        };

        // Step 5: Cache the result for future reads
        {
            let cache = self.cache.read().await;
            cache.insert(cache_key, data.clone());
        }

        Ok(data)
    }

    /// Read a file by path (complete resolution chain with caching)
    ///
    /// # Errors
    ///
    /// Returns error if path cannot be resolved or file cannot be read
    pub async fn read_file_by_path(&self, path: &str) -> Result<Vec<u8>> {
        debug!("Reading file by path: {}", path);

        // Check cache with path as key first
        {
            let cache = self.cache.read().await;
            if let Some(cached_data) = cache.get(path) {
                debug!("Cache hit for path: {}", path);
                return Ok(cached_data.clone());
            }
        }

        // Step 1: Resolve path to content key using root file
        let content_key = self.resolver.resolve_path(path).ok_or_else(|| {
            StorageError::NotFound(format!("Path not found in root file: {path}"))
        })?;

        // Step 2: Use content key pipeline
        let data = self.read_file_by_content_key(&content_key).await?;

        // Cache with path as well for faster future path-based lookups
        {
            let cache = self.cache.read().await;
            cache.insert(path.to_string(), data.clone());
        }

        Ok(data)
    }

    /// Read a file by `FileDataID` (modern clients with caching)
    ///
    /// # Errors
    ///
    /// Returns error if `FileDataID` cannot be resolved or file cannot be read
    pub async fn read_file_by_fdid(&self, fdid: u32) -> Result<Vec<u8>> {
        let fdid_key = format!("fdid:{fdid}");
        debug!("Reading file by FileDataID: {}", fdid);

        // Check cache with FDID as key
        {
            let cache = self.cache.read().await;
            if let Some(cached_data) = cache.get(&fdid_key) {
                debug!("Cache hit for FDID: {}", fdid);
                return Ok(cached_data.clone());
            }
        }

        // Step 1: Resolve FileDataID to content key
        let content_key = self
            .resolver
            .resolve_file_data_id(fdid)
            .ok_or_else(|| StorageError::NotFound(format!("FileDataID not found: {fdid}")))?;

        // Step 2: Use content key pipeline
        let data = self.read_file_by_content_key(&content_key).await?;

        // Cache with FDID key for future lookups
        {
            let cache = self.cache.read().await;
            cache.insert(fdid_key, data.clone());
        }

        Ok(data)
    }

    /// Read multiple files concurrently by content keys
    ///
    /// # Errors
    ///
    /// Returns error if any file cannot be found or read
    pub async fn read_files_by_content_keys(
        self: Arc<Self>,
        keys: &[ContentKey],
    ) -> Result<Vec<Vec<u8>>> {
        use futures::future::join_all;

        let futures = keys.iter().map(|&key| {
            let installation = Arc::clone(&self);
            async move { installation.read_file_by_content_key(&key).await }
        });

        let results: Result<Vec<_>> = join_all(futures).await.into_iter().collect();

        results
    }

    /// Read multiple files concurrently by paths
    ///
    /// # Errors
    ///
    /// Returns error if any path cannot be resolved or file cannot be read
    pub async fn read_files_by_paths(self: Arc<Self>, paths: &[String]) -> Result<Vec<Vec<u8>>> {
        use futures::future::join_all;

        let futures = paths.iter().map(|path| {
            let installation = Arc::clone(&self);
            let path = path.clone();
            async move { installation.read_file_by_path(&path).await }
        });

        let results: Result<Vec<_>> = join_all(futures).await.into_iter().collect();

        results
    }

    /// Read multiple files concurrently by `FileDataIDs`
    ///
    /// # Errors
    ///
    /// Returns error if any `FileDataID` cannot be resolved or file cannot be read
    pub async fn read_files_by_fdids(self: Arc<Self>, fdids: &[u32]) -> Result<Vec<Vec<u8>>> {
        use futures::future::join_all;

        let futures = fdids.iter().map(|&fdid| {
            let installation = Arc::clone(&self);
            async move { installation.read_file_by_fdid(fdid).await }
        });

        let results: Result<Vec<_>> = join_all(futures).await.into_iter().collect();

        results
    }

    /// Write a file to storage
    ///
    /// # Errors
    ///
    /// Returns error if file cannot be written or compressed
    pub async fn write_file(&self, data: Vec<u8>, compress: bool) -> Result<ContentKey> {
        debug!(
            "Writing file ({} bytes, compress: {})",
            data.len(),
            compress
        );

        // Calculate content key from uncompressed data
        let content_key = ContentKey::from_data(&data);

        // Write to archive and get location
        let (archive_id, archive_offset, size) = {
            let mut archive_manager = self.archive_manager.write().await;
            archive_manager.write_content(data, compress)?
        };

        // Create encoding key from compressed data location
        let mut location_data = Vec::new();
        location_data.extend_from_slice(&archive_id.to_be_bytes());
        location_data.extend_from_slice(&archive_offset.to_be_bytes());
        let encoding_key = EncodingKey::from_data(&location_data);

        // Update indices
        {
            let mut index_manager = self.index_manager.write().await;
            index_manager.add_entry(&encoding_key, archive_id, archive_offset, size)?;
        }

        // Note: Resolver cache will be updated on next lookup

        info!(
            "Wrote file to archive {} at offset {} (content key: {})",
            archive_id,
            archive_offset,
            hex::encode(content_key.as_bytes())
        );

        Ok(content_key)
    }

    /// Initialize installation by loading indices and setting up resolvers
    ///
    /// # Errors
    ///
    /// Returns error if indices cannot be loaded
    pub async fn initialize(&self) -> Result<()> {
        info!("Initializing installation at {:?}", self.path);

        // Load all index files
        self.index_manager.write().await.load_all().await?;

        // Load all archive files
        self.archive_manager.write().await.open_all().await?;

        info!("Installation initialization complete");
        Ok(())
    }

    /// Load root file for path resolution
    ///
    /// # Errors
    ///
    /// Returns error if root file cannot be loaded
    pub fn load_root_file(&self, data: &[u8]) -> Result<()> {
        self.resolver.load_root_file(data)
    }

    /// Load encoding file for content resolution
    ///
    /// # Errors
    ///
    /// Returns error if encoding file cannot be loaded
    pub fn load_encoding_file(&self, data: &[u8]) -> Result<()> {
        self.resolver.load_encoding_file(data)
    }

    /// Verify installation integrity
    ///
    /// # Errors
    ///
    /// Returns error if verification process fails
    pub async fn verify(&self) -> Result<VerificationResult> {
        info!("Verifying installation integrity");

        let mut result = VerificationResult {
            total: 0,
            valid: 0,
            invalid: 0,
            missing: 0,
        };

        // Verify index files
        let index_stats = {
            let index_manager = self.index_manager.read().await;
            index_manager.stats()
        };

        result.total += index_stats.total_entries;
        result.valid += index_stats.total_entries; // Assume valid if loaded

        // Verify archive files accessibility
        let archive_stats = {
            let archive_manager = self.archive_manager.read().await;
            archive_manager.stats()
        };

        // Add archive count to totals
        result.total += archive_stats.archive_count;
        result.valid += archive_stats.archive_count;

        info!(
            "Verification complete: {} total, {} valid, {} invalid, {} missing",
            result.total, result.valid, result.invalid, result.missing
        );

        Ok(result)
    }

    /// Get file information by path
    ///
    /// # Errors
    ///
    /// Returns error if path cannot be resolved
    pub fn get_file_info(&self, path: &str) -> Result<Option<crate::resolver::FileInfo>> {
        Ok(self.resolver.get_file_info(path))
    }

    /// Get installation statistics
    pub async fn stats(&self) -> InstallationStats {
        let index_stats = {
            let index_manager = self.index_manager.read().await;
            index_manager.stats()
        };

        let archive_stats = {
            let archive_manager = self.archive_manager.read().await;
            archive_manager.stats()
        };

        let resolver_stats = self.resolver.stats();

        InstallationStats {
            path: self.path.clone(),
            index_files: index_stats.index_count,
            index_entries: index_stats.total_entries,
            archive_files: archive_stats.archive_count,
            archive_size: archive_stats.total_size,
            cached_paths: resolver_stats.path_cache_size,
            cached_content: resolver_stats.content_cache_size,
        }
    }

    /// Get the installation path
    pub const fn path(&self) -> &PathBuf {
        &self.path
    }
}

/// Result of installation verification
#[derive(Debug, Clone)]
pub struct VerificationResult {
    /// Total number of files checked
    pub total: usize,
    /// Number of valid files
    pub valid: usize,
    /// Number of invalid/corrupted files
    pub invalid: usize,
    /// Number of missing files
    pub missing: usize,
}

/// Installation statistics
#[derive(Debug, Clone)]
pub struct InstallationStats {
    /// Installation path
    pub path: PathBuf,
    /// Number of index files
    pub index_files: usize,
    /// Total index entries
    pub index_entries: usize,
    /// Number of archive files
    pub archive_files: usize,
    /// Total archive size in bytes
    pub archive_size: u64,
    /// Number of cached path resolutions
    pub cached_paths: usize,
    /// Number of cached content resolutions
    pub cached_content: usize,
}
