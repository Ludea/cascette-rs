//! Adapter for integrating cascette-import providers with FileDataID resolution

// Basic imports that are always needed
// Import-specific types are imported within the feature-gated module

/// Configuration for the listfile provider adapter
#[derive(Debug, Clone)]
pub struct AdapterConfig {
    /// Maximum number of entries to cache
    pub cache_capacity: usize,
    /// Batch size for loading operations
    pub batch_size: u32,
    /// Whether to preload data on initialization
    pub preload_on_init: bool,
    /// Number of entries to preload
    pub preload_count: u32,
    /// Enable bidirectional caching (path->id)
    pub enable_path_cache: bool,
}

impl Default for AdapterConfig {
    fn default() -> Self {
        Self {
            cache_capacity: 50_000,
            batch_size: 1000,
            preload_on_init: false,
            preload_count: 10_000,
            enable_path_cache: true,
        }
    }
}

// Feature-gated implementation when cascette-import is available
#[cfg(feature = "import")]
mod import_impl {
    use super::AdapterConfig;
    use crate::fdid::{
        FileDataIdError, FileDataIdMapping, FileDataIdProvider, FileDataIdResult,
        ProviderCapabilities, ProviderInfo, ResolutionStats, SourceType,
    };
    use async_trait::async_trait;
    use cascette_import::listfile::{
        ListfileProvider as ImportListfileProvider, create_listfile_provider,
    };
    use cascette_import::providers::ImportProvider;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// Adapter that wraps cascette-import providers for FileDataID resolution
    ///
    /// This adapter bridges the gap between the generic `ImportProvider` trait
    /// and the specialized `FileDataIdProvider` trait, enabling seamless integration
    /// of community data sources like the WoWDev listfile.
    #[derive(Debug)]
    pub struct ListfileProviderAdapter<T: ImportProvider> {
        /// The underlying import provider
        provider: Arc<RwLock<T>>,
        /// Cached mappings for efficient lookup
        _cache: Arc<RwLock<FileDataIdCache>>,
        /// Configuration for the adapter
        config: AdapterConfig,
        /// Runtime statistics
        _stats: Arc<RwLock<AdapterStats>>,
    }

    impl<T: ImportProvider> ListfileProviderAdapter<T> {
        /// Create a new adapter with the given import provider
        pub fn new(provider: T) -> Self {
            Self {
                provider: Arc::new(RwLock::new(provider)),
                _cache: Arc::new(RwLock::new(FileDataIdCache::new())),
                config: AdapterConfig::default(),
                _stats: Arc::new(RwLock::new(AdapterStats::default())),
            }
        }

        /// Create a new adapter with custom configuration
        pub fn with_config(provider: T, config: AdapterConfig) -> Self {
            Self {
                provider: Arc::new(RwLock::new(provider)),
                _cache: Arc::new(RwLock::new(FileDataIdCache::with_capacity(
                    config.cache_capacity,
                ))),
                config,
                _stats: Arc::new(RwLock::new(AdapterStats::default())),
            }
        }
    }

    impl ListfileProviderAdapter<ImportListfileProvider> {
        /// Create a new adapter specifically for WoWDev listfile provider
        pub async fn new_listfile_provider() -> FileDataIdResult<Self> {
            let provider = create_listfile_provider()
                .await
                .map_err(FileDataIdError::ImportProvider)?;
            Ok(Self::new(provider))
        }

        /// Create a new adapter with custom configuration for WoWDev listfile provider
        pub async fn new_listfile_provider_with_config(
            config: AdapterConfig,
        ) -> FileDataIdResult<Self> {
            let provider = create_listfile_provider()
                .await
                .map_err(FileDataIdError::ImportProvider)?;
            Ok(Self::with_config(provider, config))
        }
    }

    #[async_trait]
    impl<T: ImportProvider> FileDataIdProvider for ListfileProviderAdapter<T> {
        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                name: "Listfile Provider Adapter".to_string(),
                description: "Adapter for cascette-import listfile providers".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                source_type: SourceType::Listfile,
                capabilities: ProviderCapabilities {
                    id_to_path: true,
                    path_to_id: true,
                    search: false,
                    enumeration: true,
                    batch_operations: true,
                    real_time_updates: false,
                    max_batch_size: Some(self.config.batch_size as usize),
                },
                last_updated: None,
                metadata: HashMap::new(),
            }
        }

        async fn initialize(&mut self) -> FileDataIdResult<()> {
            let mut provider = self.provider.write().await;
            provider
                .initialize()
                .await
                .map_err(FileDataIdError::ImportProvider)?;
            Ok(())
        }

        async fn is_available(&self) -> bool {
            let provider = self.provider.read().await;
            provider.is_available().await
        }

        async fn resolve_id(&self, id: u32) -> FileDataIdResult<Option<String>> {
            let provider = self.provider.read().await;
            provider
                .resolve_file_id(id)
                .await
                .map_err(FileDataIdError::ImportProvider)
        }

        async fn resolve_path(&self, _path: &str) -> FileDataIdResult<Option<u32>> {
            // Path->ID lookups are not efficiently supported by ImportProvider
            Ok(None)
        }

        async fn get_mapping(&self, id: u32) -> FileDataIdResult<Option<FileDataIdMapping>> {
            if let Some(path) = self.resolve_id(id).await? {
                Ok(Some(FileDataIdMapping::new(id, path)))
            } else {
                Ok(None)
            }
        }

        async fn get_mappings(&self, ids: &[u32]) -> FileDataIdResult<Vec<FileDataIdMapping>> {
            let provider = self.provider.read().await;
            let file_mappings = provider
                .get_file_mappings(ids)
                .await
                .map_err(FileDataIdError::ImportProvider)?;

            let mut result = Vec::new();
            for file_mapping in file_mappings {
                let mapping = FileDataIdMapping::new(file_mapping.file_id, file_mapping.path);
                result.push(mapping);
            }

            Ok(result)
        }

        async fn list_ids(&self) -> FileDataIdResult<Vec<u32>> {
            // Use the trait method to get all IDs from the provider
            let provider = self.provider.read().await;
            Ok(provider.get_all_file_ids())
        }

        async fn mapping_count(&self) -> FileDataIdResult<usize> {
            // Cannot be efficiently determined from ImportProvider
            Ok(0)
        }

        async fn contains_id(&self, id: u32) -> FileDataIdResult<bool> {
            Ok(self.resolve_id(id).await?.is_some())
        }

        async fn contains_path(&self, _path: &str) -> FileDataIdResult<bool> {
            // Not efficiently supported
            Ok(false)
        }

        async fn refresh(&mut self) -> FileDataIdResult<usize> {
            let provider = self.provider.read().await;
            provider
                .refresh_cache()
                .await
                .map_err(FileDataIdError::ImportProvider)?;
            Ok(0)
        }

        async fn stats(&self) -> FileDataIdResult<ResolutionStats> {
            Ok(ResolutionStats::default())
        }
    }

    /// Internal cache for FileDataID mappings
    #[derive(Debug)]
    struct FileDataIdCache {
        /// ID to mapping lookup
        _id_to_mapping: HashMap<u32, FileDataIdMapping>,
    }

    impl FileDataIdCache {
        fn new() -> Self {
            Self {
                _id_to_mapping: HashMap::new(),
            }
        }

        fn with_capacity(capacity: usize) -> Self {
            Self {
                _id_to_mapping: HashMap::with_capacity(capacity),
            }
        }
    }

    /// Runtime statistics for the adapter
    #[derive(Debug, Default)]
    struct AdapterStats {
        /// Cache hits
        _cache_hits: u64,
        /// Cache misses
        _cache_misses: u64,
    }
}

// Re-export the implementation when the feature is enabled
#[cfg(feature = "import")]
pub use import_impl::ListfileProviderAdapter;

// Provide a compile error when the feature is disabled
#[cfg(not(feature = "import"))]
compile_error!(
    "ListfileProviderAdapter requires the 'import' feature to be enabled. Add 'import' to your cascette-metadata features in Cargo.toml"
);
