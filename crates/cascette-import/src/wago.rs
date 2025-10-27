//! wago.tools integration for World of Warcraft build information

use crate::error::{ImportError, ImportResult};
use crate::providers::{CacheStats, DataSource, ImportProvider, ImportProviderInfo};
use crate::types::{BuildInfo, ProviderCapabilities};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, info, warn};

/// wago.tools API base URL
const WAGO_API_BASE: &str = "https://wago.tools/api/builds";

/// wago.tools provider for build information
pub struct WagoProvider {
    info: ImportProviderInfo,
    client: Client,
    cache_dir: PathBuf,
    build_cache: HashMap<String, Vec<WagoBuild>>,
    last_refresh: Option<chrono::DateTime<chrono::Utc>>,
}

/// Build information from wago.tools API
#[derive(Debug, Clone, Serialize, Deserialize)]
struct WagoBuild {
    product: String,
    version: String,
    created_at: String,
    build_config: String,
    product_config: Option<String>,
    cdn_config: String,
    is_bgdl: bool,
    #[serde(default)]
    seqn: Option<u32>,
}

impl WagoProvider {
    /// Create a new wago.tools provider with default cache directory
    pub async fn new() -> ImportResult<Self> {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("cascette-import")
            .join("wago");

        Self::with_cache_dir(cache_dir).await
    }

    /// Create provider with custom cache directory
    pub async fn with_cache_dir(cache_dir: PathBuf) -> ImportResult<Self> {
        let info = ImportProviderInfo {
            source: DataSource::Wago,
            name: "wago.tools".to_string(),
            version: "1.0.0".to_string(),
            description: "wago.tools community build archive".to_string(),
            endpoint: Some(WAGO_API_BASE.to_string()),
            capabilities: ProviderCapabilities {
                builds: true,
                file_mappings: false,
                real_time: false,
                requires_auth: false,
            },
            rate_limit: Some(60),                 // 60 requests per minute
            cache_ttl: Duration::from_secs(3600), // 1 hour
        };

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(format!("cascette-import/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(ImportError::Network)?;

        let mut provider = Self {
            info,
            client,
            cache_dir,
            build_cache: HashMap::new(),
            last_refresh: None,
        };

        // Try to load from cache, but don't fail if it doesn't exist
        if let Err(e) = provider.load_from_cache().await {
            debug!("Failed to load cache: {}", e);
        }

        Ok(provider)
    }

    /// Fetch builds from the wago.tools API
    async fn fetch_builds(&self) -> ImportResult<Vec<WagoBuild>> {
        debug!("Fetching builds from wago.tools API");

        let url = WAGO_API_BASE;
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(ImportError::Network)?;

        if !response.status().is_success() {
            return Err(ImportError::HttpStatus {
                provider: "wago.tools".to_string(),
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        // Get the raw text first to debug any issues
        let text = response.text().await.map_err(|e| ImportError::Provider {
            provider: "wago.tools".to_string(),
            message: format!("Failed to read response text: {}", e),
        })?;

        debug!(
            "Raw response (first 200 chars): {}",
            &text.chars().take(200).collect::<String>()
        );

        // Parse response as a map of product -> builds
        let products_map: HashMap<String, Vec<WagoBuild>> =
            serde_json::from_str(&text).map_err(|e| ImportError::Provider {
                provider: "wago.tools".to_string(),
                message: format!(
                    "Failed to parse JSON response: {} | Response: {}",
                    e,
                    &text.chars().take(500).collect::<String>()
                ),
            })?;

        // Flatten all builds from all products
        let mut all_builds = Vec::new();
        for (_, builds) in products_map {
            all_builds.extend(builds);
        }

        info!("Fetched {} builds from wago.tools", all_builds.len());
        Ok(all_builds)
    }

    /// Convert wago build to our `BuildInfo` format
    fn convert_build(wago_build: &WagoBuild) -> BuildInfo {
        let mut metadata = HashMap::new();
        metadata.insert("build_config".to_string(), wago_build.build_config.clone());
        metadata.insert("cdn_config".to_string(), wago_build.cdn_config.clone());
        if let Some(product_config) = &wago_build.product_config {
            metadata.insert("product_config".to_string(), product_config.clone());
        }
        metadata.insert("is_bgdl".to_string(), wago_build.is_bgdl.to_string());

        if let Some(seqn) = wago_build.seqn {
            metadata.insert("seqn".to_string(), seqn.to_string());
        }

        // Parse build number from version string (e.g., "11.2.5.62785" -> 62785)
        let build_number = wago_build
            .version
            .split('.')
            .next_back()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);

        // Parse timestamp from created_at
        let timestamp =
            chrono::NaiveDateTime::parse_from_str(&wago_build.created_at, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|dt| chrono::DateTime::from_naive_utc_and_offset(dt, chrono::Utc));

        // Determine version type based on product code
        let version_type =
            if wago_build.product.contains("_ptr") || wago_build.product.ends_with('t') {
                "ptr"
            } else if wago_build.product.contains("_beta") {
                "beta"
            } else {
                "live"
            }
            .to_string();

        BuildInfo {
            product: wago_build.product.to_lowercase(),
            version: wago_build.version.clone(),
            build: build_number,
            version_type,
            region: None, // wago.tools doesn't provide region info
            timestamp,
            metadata,
        }
    }

    /// Load builds from disk cache
    async fn load_from_cache(&mut self) -> ImportResult<()> {
        let cache_file = self.cache_dir.join("builds.json");

        if cache_file.exists() {
            debug!("Loading builds from cache file: {}", cache_file.display());

            let content = tokio::fs::read_to_string(&cache_file)
                .await
                .map_err(ImportError::Io)?;

            let cached_data: HashMap<String, (Vec<WagoBuild>, chrono::DateTime<chrono::Utc>)> =
                serde_json::from_str(&content).map_err(ImportError::Json)?;

            // Check if cache is still valid (within TTL)
            let now = chrono::Utc::now();
            let cache_ttl = chrono::Duration::seconds(
                i64::try_from(self.info.cache_ttl.as_secs()).unwrap_or(i64::MAX),
            );

            let mut valid_cache = HashMap::new();
            for (product, (builds, timestamp)) in cached_data {
                if now - timestamp < cache_ttl {
                    valid_cache.insert(product.clone(), builds);
                    self.last_refresh = Some(timestamp);
                } else {
                    debug!("Cache for product {} is stale", product);
                }
            }

            self.build_cache = valid_cache;
            info!("Loaded {} products from cache", self.build_cache.len());
        } else {
            debug!("No cache file found at {}", cache_file.display());
        }

        Ok(())
    }

    /// Save builds to disk cache
    async fn save_to_cache(&self) -> ImportResult<()> {
        if !self.cache_dir.exists() {
            std::fs::create_dir_all(&self.cache_dir).map_err(ImportError::Io)?;
        }

        let cache_file = self.cache_dir.join("builds.json");
        let now = chrono::Utc::now();

        // Create cache data with timestamps
        let cache_data: HashMap<String, (Vec<WagoBuild>, chrono::DateTime<chrono::Utc>)> = self
            .build_cache
            .iter()
            .map(|(k, v)| (k.clone(), (v.clone(), now)))
            .collect();

        let json = serde_json::to_string_pretty(&cache_data).map_err(ImportError::Json)?;

        tokio::fs::write(&cache_file, json)
            .await
            .map_err(ImportError::Io)?;

        debug!("Saved cache to {}", cache_file.display());
        Ok(())
    }

    /// Preload builds cache from API
    async fn preload_builds_cache(&mut self) -> ImportResult<()> {
        debug!("Preloading wago.tools builds cache");

        let all_builds = self.fetch_builds().await?;

        // Group builds by product
        let mut product_cache: HashMap<String, Vec<WagoBuild>> = HashMap::new();
        for build in all_builds {
            let product = build.product.to_lowercase();
            product_cache.entry(product).or_default().push(build);
        }

        self.build_cache = product_cache;
        self.last_refresh = Some(chrono::Utc::now());

        info!(
            "Preloaded {} products to wago.tools cache",
            self.build_cache.len()
        );
        Ok(())
    }

    /// Clear the cache
    #[allow(dead_code)]
    async fn clear_cache(&mut self) -> ImportResult<()> {
        self.build_cache.clear();
        self.last_refresh = None;

        // Delete cache file
        let cache_file = self.cache_dir.join("builds.json");
        if cache_file.exists() {
            tokio::fs::remove_file(&cache_file)
                .await
                .map_err(ImportError::Io)?;
        }

        Ok(())
    }
}

#[async_trait]
impl ImportProvider for WagoProvider {
    fn info(&self) -> &ImportProviderInfo {
        &self.info
    }

    async fn initialize(&mut self) -> ImportResult<()> {
        // Create cache directory if it doesn't exist
        if !self.cache_dir.exists() {
            std::fs::create_dir_all(&self.cache_dir).map_err(ImportError::Io)?;
        }

        // Try to load from cache
        if let Err(e) = self.load_from_cache().await {
            warn!("Failed to load wago.tools cache: {}", e);
        }

        Ok(())
    }

    async fn get_builds(&self, product: &str) -> ImportResult<Vec<BuildInfo>> {
        let product_key = product.to_lowercase();

        // Check if we have cached data for this product
        if let Some(cached_builds) = self.build_cache.get(&product_key) {
            debug!("Using cached builds for product: {}", product_key);
            let builds: Vec<BuildInfo> = cached_builds.iter().map(Self::convert_build).collect();
            return Ok(builds);
        }

        // Need to fetch all builds and filter for the requested product
        // This is due to the API structure returning all products together
        let all_builds = self.fetch_builds().await?;

        // Group builds by product and update cache
        let mut product_cache: HashMap<String, Vec<WagoBuild>> = HashMap::new();
        for build in all_builds {
            let product = build.product.to_lowercase();
            product_cache.entry(product).or_default().push(build);
        }

        // This is a read-only operation, but we need to indicate cache should be updated
        // The actual cache update would need &mut self
        debug!("Would update cache with {} products", product_cache.len());

        // Find builds for the requested product
        let product_builds = product_cache.get(&product_key).cloned().unwrap_or_default();

        // Convert to our format
        let builds: Vec<BuildInfo> = product_builds.iter().map(Self::convert_build).collect();
        Ok(builds)
    }

    async fn is_available(&self) -> bool {
        // Check if API is accessible with a quick timeout
        let response = self
            .client
            .get(WAGO_API_BASE)
            .timeout(Duration::from_secs(5))
            .send()
            .await;

        match response {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    async fn refresh_cache(&self) -> ImportResult<()> {
        debug!("Would refresh wago.tools cache");

        // Fetch fresh data to validate the API is working
        let all_builds = self.fetch_builds().await?;

        // This is a read-only operation - we can't modify self
        // In a real implementation, this would need interior mutability
        info!("Would refresh cache with {} builds", all_builds.len());
        Ok(())
    }

    async fn cache_stats(&self) -> ImportResult<CacheStats> {
        let total_builds: usize = self.build_cache.values().map(std::vec::Vec::len).sum();

        Ok(CacheStats {
            entries: total_builds,
            hits: 0,   // Would need to track in real implementation
            misses: 0, // Would need to track in real implementation
            last_refresh: self.last_refresh,
            size_bytes: total_builds * std::mem::size_of::<WagoBuild>(), // Rough estimate
        })
    }
}

/// Check if the wago.tools cache needs refresh
fn wago_cache_needs_refresh(provider: &WagoProvider) -> bool {
    if provider.build_cache.is_empty() {
        return true;
    }

    match provider.last_refresh {
        Some(last) => {
            let elapsed = chrono::Utc::now() - last;
            let cache_ttl = chrono::Duration::seconds(
                i64::try_from(provider.info.cache_ttl.as_secs()).unwrap_or(i64::MAX),
            );
            elapsed > cache_ttl
        }
        None => true,
    }
}

/// Attempt to refresh wago.tools cache and save to disk
async fn try_refresh_wago_cache(provider: &mut WagoProvider) {
    debug!("Cache is stale or empty, refreshing wago.tools data");

    match provider.preload_builds_cache().await {
        Ok(()) => {
            if let Err(e) = provider.save_to_cache().await {
                warn!("Failed to save wago.tools cache to disk: {}", e);
            }
        }
        Err(e) => {
            warn!("Failed to refresh wago.tools cache: {}", e);
            // Continue anyway - provider can work with existing cache
        }
    }
}

/// Initialize provider and refresh cache if needed
async fn initialize_wago_provider(mut provider: WagoProvider) -> ImportResult<WagoProvider> {
    if wago_cache_needs_refresh(&provider) {
        if provider.is_available().await {
            try_refresh_wago_cache(&mut provider).await;
        } else {
            warn!("wago.tools API not available and cache is stale");
        }
    } else {
        debug!("Using existing wago.tools cache (still valid)");
    }
    Ok(provider)
}

/// Create a new wago.tools provider
pub async fn create_wago_provider() -> ImportResult<WagoProvider> {
    let provider = WagoProvider::new().await?;
    let provider = initialize_wago_provider(provider).await?;
    info!("wago.tools provider created");
    Ok(provider)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_provider_creation() {
        let provider = WagoProvider::new().await;
        assert!(provider.is_ok());
    }

    #[tokio::test]
    async fn test_provider_info() {
        let provider = WagoProvider::new()
            .await
            .expect("Failed to create WagoProvider");
        let info = provider.info();
        assert_eq!(info.name, "wago.tools");
        assert!(info.capabilities.builds);
        assert!(!info.capabilities.file_mappings);
    }
}
