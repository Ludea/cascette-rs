//! Import manager for coordinating multiple data providers

use crate::error::{ImportError, ImportResult};
use crate::providers::{BuildSearchCriteria, CacheStats, DataSource, ImportProvider};
use crate::types::BuildInfo;
use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

/// Configuration for the import manager
#[derive(Debug, Clone)]
pub struct ImportManagerConfig {
    /// Default cache TTL for providers
    pub default_cache_ttl: Duration,

    /// Maximum concurrent provider operations
    pub max_concurrent_ops: usize,

    /// Enable provider failover (try next provider on error)
    pub enable_failover: bool,

    /// Request timeout for individual providers
    pub request_timeout: Duration,

    /// Enable provider health checking
    pub enable_health_checks: bool,

    /// Health check interval
    pub health_check_interval: Duration,
}

impl Default for ImportManagerConfig {
    fn default() -> Self {
        Self {
            default_cache_ttl: Duration::from_secs(24 * 3600), // 24 hours
            max_concurrent_ops: 10,
            enable_failover: true,
            request_timeout: Duration::from_secs(30),
            enable_health_checks: true,
            health_check_interval: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// Import manager that coordinates multiple data providers
pub struct ImportManager {
    /// Registered providers by name
    providers: DashMap<String, Box<dyn ImportProvider>>,

    /// Provider health status
    provider_health: Arc<RwLock<HashMap<String, bool>>>,

    /// Manager configuration
    #[allow(dead_code)]
    config: ImportManagerConfig,

    /// Aggregated cache for cross-provider data
    build_cache: Arc<RwLock<HashMap<String, Vec<BuildInfo>>>>,
    file_mapping_cache: Arc<RwLock<HashMap<u32, String>>>,
}

impl ImportManager {
    /// Create a new import manager with default configuration
    pub fn new() -> Self {
        Self::with_config(ImportManagerConfig::default())
    }

    /// Create a new import manager with custom configuration
    pub fn with_config(config: ImportManagerConfig) -> Self {
        Self {
            providers: DashMap::new(),
            provider_health: Arc::new(RwLock::new(HashMap::new())),
            config,
            build_cache: Arc::new(RwLock::new(HashMap::new())),
            file_mapping_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a provider to the manager
    pub async fn add_provider(
        &self,
        name: &str,
        mut provider: Box<dyn ImportProvider>,
    ) -> ImportResult<()> {
        info!("Initializing provider: {}", name);

        // Initialize the provider
        if let Err(e) = provider.initialize().await {
            error!("Failed to initialize provider {}: {}", name, e);
            return Err(e);
        }

        // Check availability
        let is_available = provider.is_available().await;
        info!("Provider {} availability: {}", name, is_available);

        // Update health status
        {
            let mut health = self.provider_health.write().await;
            health.insert(name.to_string(), is_available);
        }

        // Register provider
        self.providers.insert(name.to_string(), provider);

        info!("Provider {} registered successfully", name);
        Ok(())
    }

    /// Remove a provider from the manager
    pub async fn remove_provider(&self, name: &str) -> ImportResult<()> {
        if self.providers.remove(name).is_some() {
            let mut health = self.provider_health.write().await;
            health.remove(name);
            info!("Provider {} removed", name);
            Ok(())
        } else {
            Err(ImportError::NotFound(format!("Provider: {}", name)))
        }
    }

    /// Get list of registered provider names
    pub fn list_providers(&self) -> Vec<String> {
        self.providers
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Check if a provider is registered
    pub fn has_provider(&self, name: &str) -> bool {
        self.providers.contains_key(name)
    }

    /// Get provider health status
    pub async fn get_provider_health(&self, name: &str) -> Option<bool> {
        let health = self.provider_health.read().await;
        health.get(name).copied()
    }

    /// Get builds for a product, trying all available providers
    pub async fn get_builds(&self, product: &str) -> ImportResult<Vec<BuildInfo>> {
        // Check cache first
        let cache_key = product.to_string();
        {
            let cache = self.build_cache.read().await;
            if let Some(cached_builds) = cache.get(&cache_key) {
                return Ok(cached_builds.clone());
            }
        }

        let mut all_builds = Vec::new();
        let mut errors = Vec::new();

        // Try each provider
        for provider_entry in self.providers.iter() {
            let provider_name = provider_entry.key();
            let provider = provider_entry.value();

            // Skip unhealthy providers
            if self.get_provider_health(provider_name).await == Some(false) {
                continue;
            }

            match provider.get_builds(product).await {
                Ok(mut builds) => {
                    info!(
                        "Provider {} returned {} builds for {}",
                        provider_name,
                        builds.len(),
                        product
                    );
                    all_builds.append(&mut builds);
                }
                Err(e) => {
                    warn!(
                        "Provider {} failed for product {}: {}",
                        provider_name, product, e
                    );
                    errors.push((provider_name.clone(), e));
                }
            }
        }

        if all_builds.is_empty() && !errors.is_empty() {
            // All providers failed
            let error_msg = errors
                .iter()
                .map(|(name, err)| format!("{}: {}", name, err))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ImportError::Provider {
                provider: "all".to_string(),
                message: format!("All providers failed: {}", error_msg),
            });
        }

        // Deduplicate builds by version + build number
        all_builds.sort_by(|a, b| {
            a.product
                .cmp(&b.product)
                .then(a.version.cmp(&b.version))
                .then(a.build.cmp(&b.build))
        });
        all_builds.dedup_by(|a, b| {
            a.product == b.product && a.version == b.version && a.build == b.build
        });

        // Cache results
        {
            let mut cache = self.build_cache.write().await;
            cache.insert(cache_key, all_builds.clone());
        }

        Ok(all_builds)
    }

    /// Get all builds from all providers
    pub async fn get_all_builds(&self) -> ImportResult<Vec<BuildInfo>> {
        let mut all_builds = Vec::new();

        for provider_entry in self.providers.iter() {
            let provider_name = provider_entry.key();
            let provider = provider_entry.value();

            // Skip unhealthy providers
            if self.get_provider_health(provider_name).await == Some(false) {
                continue;
            }

            match provider.get_all_builds().await {
                Ok(mut builds) => {
                    info!(
                        "Provider {} returned {} total builds",
                        provider_name,
                        builds.len()
                    );
                    all_builds.append(&mut builds);
                }
                Err(e) => {
                    warn!("Provider {} failed to get all builds: {}", provider_name, e);
                }
            }
        }

        // Deduplicate and sort
        all_builds.sort_by(|a, b| {
            a.product.cmp(&b.product).then(b.build.cmp(&a.build)) // Newer builds first
        });
        all_builds.dedup_by(|a, b| {
            a.product == b.product && a.version == b.version && a.build == b.build
        });

        Ok(all_builds)
    }

    /// Resolve file ID to path using available listfile providers
    pub async fn resolve_file_id(&self, file_id: u32) -> ImportResult<Option<String>> {
        // Check cache first
        {
            let cache = self.file_mapping_cache.read().await;
            if let Some(path) = cache.get(&file_id) {
                return Ok(Some(path.clone()));
            }
        }

        // Try providers that support file mappings
        for provider_entry in self.providers.iter() {
            let provider_name = provider_entry.key();
            let provider = provider_entry.value();

            // Skip unhealthy providers
            if self.get_provider_health(provider_name).await == Some(false) {
                continue;
            }

            // Only try providers that support file mappings
            if !provider.info().capabilities.file_mappings {
                continue;
            }

            match provider.resolve_file_id(file_id).await {
                Ok(Some(path)) => {
                    // Cache the result
                    {
                        let mut cache = self.file_mapping_cache.write().await;
                        cache.insert(file_id, path.clone());
                    }
                    return Ok(Some(path));
                }
                Ok(None) => {} // Try next provider
                Err(e) => {
                    warn!(
                        "Provider {} failed to resolve file ID {}: {}",
                        provider_name, file_id, e
                    );
                }
            }
        }

        Ok(None)
    }

    /// Search builds across all providers
    pub async fn search_builds(
        &self,
        criteria: &BuildSearchCriteria,
    ) -> ImportResult<Vec<BuildInfo>> {
        let mut results = Vec::new();

        for provider_entry in self.providers.iter() {
            let provider_name = provider_entry.key();
            let provider = provider_entry.value();

            // Skip unhealthy providers
            if self.get_provider_health(provider_name).await == Some(false) {
                continue;
            }

            // Only try providers that support builds
            if !provider.info().capabilities.builds {
                continue;
            }

            match provider.search_builds(criteria).await {
                Ok(mut builds) => {
                    info!(
                        "Provider {} found {} matching builds",
                        provider_name,
                        builds.len()
                    );
                    results.append(&mut builds);
                }
                Err(e) => {
                    warn!("Provider {} failed build search: {}", provider_name, e);
                }
            }
        }

        // Deduplicate results
        results.sort_by(|a, b| {
            a.product.cmp(&b.product).then(b.build.cmp(&a.build)) // Newer builds first
        });
        results.dedup_by(|a, b| {
            a.product == b.product && a.version == b.version && a.build == b.build
        });

        Ok(results)
    }

    /// Refresh all provider caches
    pub async fn refresh_all_caches(&self) -> ImportResult<()> {
        // Clear local caches
        {
            let mut build_cache = self.build_cache.write().await;
            build_cache.clear();
        }
        {
            let mut file_cache = self.file_mapping_cache.write().await;
            file_cache.clear();
        }

        // Refresh provider caches
        for provider_entry in self.providers.iter() {
            let provider_name = provider_entry.key();
            let provider = provider_entry.value();

            match provider.refresh_cache().await {
                Ok(()) => info!("Refreshed cache for provider {}", provider_name),
                Err(e) => warn!(
                    "Failed to refresh cache for provider {}: {}",
                    provider_name, e
                ),
            }
        }

        Ok(())
    }

    /// Get aggregated cache statistics
    pub async fn get_cache_stats(&self) -> HashMap<String, CacheStats> {
        let mut stats = HashMap::new();

        for provider_entry in self.providers.iter() {
            let provider_name = provider_entry.key().clone();
            let provider = provider_entry.value();

            match provider.cache_stats().await {
                Ok(provider_stats) => {
                    stats.insert(provider_name, provider_stats);
                }
                Err(e) => {
                    warn!(
                        "Failed to get cache stats for provider {}: {}",
                        provider_name, e
                    );
                }
            }
        }

        stats
    }

    /// Perform health checks on all providers
    pub async fn health_check_all(&self) -> HashMap<String, bool> {
        let mut health_status = HashMap::new();

        for provider_entry in self.providers.iter() {
            let provider_name = provider_entry.key().clone();
            let provider = provider_entry.value();

            let is_healthy = provider.is_available().await;
            health_status.insert(provider_name, is_healthy);
        }

        // Update cached health status
        {
            let mut health = self.provider_health.write().await;
            health.extend(health_status.iter().map(|(k, &v)| (k.clone(), v)));
        }

        health_status
    }

    /// Get providers that support a specific data source type
    pub fn get_providers_for_source(&self, source: DataSource) -> Vec<String> {
        let mut matching_providers = Vec::new();

        for provider_entry in self.providers.iter() {
            let provider_name = provider_entry.key().clone();
            let provider = provider_entry.value();

            if provider.info().source == source {
                matching_providers.push(provider_name);
            }
        }

        matching_providers
    }
}

impl Default for ImportManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::providers::ImportProviderInfo;
    use crate::types::ProviderCapabilities;
    use async_trait::async_trait;
    use std::time::Duration;

    struct MockProvider {
        info: ImportProviderInfo,
        available: bool,
    }

    impl MockProvider {
        fn new(source: DataSource, name: &str) -> Self {
            Self {
                info: ImportProviderInfo {
                    source,
                    name: name.to_string(),
                    description: "Mock provider for testing".to_string(),
                    version: "1.0.0".to_string(),
                    endpoint: None,
                    capabilities: ProviderCapabilities {
                        builds: true,
                        file_mappings: false,
                        real_time: false,
                        requires_auth: false,
                    },
                    rate_limit: None,
                    cache_ttl: Duration::from_secs(300),
                },
                available: true,
            }
        }
    }

    #[async_trait]
    impl ImportProvider for MockProvider {
        fn info(&self) -> &ImportProviderInfo {
            &self.info
        }

        async fn initialize(&mut self) -> ImportResult<()> {
            Ok(())
        }

        async fn is_available(&self) -> bool {
            self.available
        }

        async fn get_builds(&self, product: &str) -> ImportResult<Vec<BuildInfo>> {
            Ok(vec![BuildInfo {
                product: product.to_string(),
                version: "1.0.0".to_string(),
                build: 12_345,
                version_type: "live".to_string(),
                region: None,
                timestamp: None,
                metadata: HashMap::new(),
            }])
        }
    }

    #[tokio::test]
    async fn test_import_manager_basic_operations() {
        let manager = ImportManager::new();

        // Initially no providers
        assert_eq!(manager.list_providers().len(), 0);
        assert!(!manager.has_provider("test"));

        // Add a provider
        let provider = MockProvider::new(DataSource::Wago, "test-provider");
        manager
            .add_provider("test", Box::new(provider))
            .await
            .expect("Failed to add provider");

        // Verify provider is registered
        assert_eq!(manager.list_providers().len(), 1);
        assert!(manager.has_provider("test"));

        // Test builds functionality
        let builds = manager
            .get_builds("wow")
            .await
            .expect("Failed to get builds");
        assert!(!builds.is_empty());

        // Remove provider
        manager
            .remove_provider("test")
            .await
            .expect("Failed to remove provider");
        assert_eq!(manager.list_providers().len(), 0);
    }
}
