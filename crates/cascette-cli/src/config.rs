//! Configuration management for cascette CLI
//!
//! Handles application configuration including protocol endpoints,
//! region settings, and other runtime parameters.
#![allow(clippy::doc_link_with_quotes)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::derivable_impls)]

use anyhow::{Context, Result, bail};
use config::{Config, Environment, File};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration version for migration support
const CONFIG_VERSION: u32 = 1;

/// Main configuration structure for cascette
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CascetteConfig {
    /// Configuration version for migration support
    #[serde(default = "default_config_version")]
    version: u32,

    /// Default region for operations
    #[serde(default = "default_region")]
    pub region: String,

    /// Protocol endpoints configuration
    #[serde(default)]
    pub endpoints: EndpointsConfig,

    /// Cache configuration
    #[serde(default)]
    pub cache: CacheConfig,

    /// Network configuration
    #[serde(default)]
    pub network: NetworkConfig,

    /// CDN configuration overrides
    #[serde(default)]
    pub cdn_overrides: CdnOverridesConfig,
}

/// Protocol endpoint URLs with region template support
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EndpointsConfig {
    /// Ribbit TCP host (host:port format)
    /// Default: `{region}.version.battle.net:1119`
    #[serde(default = "default_ribbit_host")]
    pub ribbit_host: String,

    /// TACT HTTP endpoint URL template
    /// Default: `http://{region}.patch.battle.net:1119`
    #[serde(default = "default_tact_http_url")]
    pub tact_http: String,

    /// TACT HTTPS endpoint URL template
    /// Default: `https://{region}.version.battle.net`
    #[serde(default = "default_tact_https_url")]
    pub tact_https: String,
}

/// Cache configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CacheConfig {
    /// Enable caching
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Cache directory (relative to `cache_dir` if not absolute)
    #[serde(default)]
    pub directory: Option<PathBuf>,

    /// TTL for API responses in seconds (Ribbit/TACT endpoints)
    #[serde(default = "default_api_ttl")]
    pub api_ttl_seconds: u64,

    /// TTL for CDN downloads in seconds
    #[serde(default = "default_cdn_ttl")]
    pub cdn_ttl_seconds: u64,

    /// Maximum cache size in MB
    #[serde(default = "default_cache_size")]
    pub max_size_mb: u64,
}

/// Network configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct NetworkConfig {
    /// Connection timeout in seconds
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout: u64,

    /// Request timeout in seconds
    #[serde(default = "default_request_timeout")]
    pub request_timeout: u64,

    /// Number of retries for failed requests
    #[serde(default = "default_retry_count")]
    pub retry_count: u32,

    /// Enable parallel downloads
    #[serde(default = "default_true")]
    pub parallel_downloads: bool,

    /// Maximum concurrent downloads
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
}

/// CDN override configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CdnOverridesConfig {
    /// Override CDN hosts for all products
    /// When set, these hosts replace the ones returned by the cdns endpoint
    /// Format: ["cdn1.example.com", "cdn2.example.com:1119"]
    #[serde(default)]
    pub hosts: Vec<String>,

    /// Per-product CDN host overrides
    /// Key is the product code (e.g., "wow", "wow_classic")
    /// Value is a list of CDN hosts for that specific product
    #[serde(default)]
    pub product_hosts: std::collections::HashMap<String, Vec<String>>,
}

impl Default for CascetteConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            region: default_region(),
            endpoints: EndpointsConfig::default(),
            cache: CacheConfig::default(),
            network: NetworkConfig::default(),
            cdn_overrides: CdnOverridesConfig::default(),
        }
    }
}

impl Default for EndpointsConfig {
    fn default() -> Self {
        Self {
            ribbit_host: default_ribbit_host(),
            tact_http: default_tact_http_url(),
            tact_https: default_tact_https_url(),
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            directory: None,
            api_ttl_seconds: default_api_ttl(),
            cdn_ttl_seconds: default_cdn_ttl(),
            max_size_mb: default_cache_size(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            connect_timeout: default_connect_timeout(),
            request_timeout: default_request_timeout(),
            retry_count: default_retry_count(),
            parallel_downloads: true,
            max_concurrent: default_max_concurrent(),
        }
    }
}

impl Default for CdnOverridesConfig {
    fn default() -> Self {
        Self {
            hosts: Vec::new(),
            product_hosts: std::collections::HashMap::new(),
        }
    }
}

// Default value functions
fn default_config_version() -> u32 {
    CONFIG_VERSION
}

fn default_region() -> String {
    "us".to_string()
}

fn default_ribbit_host() -> String {
    "{region}.version.battle.net:1119".to_string()
}

fn default_tact_http_url() -> String {
    "http://{region}.patch.battle.net:1119".to_string()
}

fn default_tact_https_url() -> String {
    "https://{region}.version.battle.net".to_string()
}

fn default_true() -> bool {
    true
}

fn default_api_ttl() -> u64 {
    300 // 5 minutes for API responses
}

fn default_cdn_ttl() -> u64 {
    604_800 // 1 week for CDN downloads
}

fn default_cache_size() -> u64 {
    25600 // 25GB
}

fn default_connect_timeout() -> u64 {
    10
}

fn default_request_timeout() -> u64 {
    30
}

fn default_retry_count() -> u32 {
    3
}

fn default_max_concurrent() -> usize {
    4
}

impl CascetteConfig {
    /// Load configuration from default locations
    pub fn load() -> Result<Self> {
        let config_path = crate::paths::config_file()?;
        Self::load_from(config_path)
    }

    /// Load configuration from a specific file
    pub fn load_from(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let path = path.as_ref();
        let mut builder = Config::builder();

        // Start with defaults
        builder = builder.add_source(Config::try_from(&Self::default())?);

        // Add config file if it exists
        if path.exists() {
            builder = builder.add_source(File::from(path));
        }

        // Override with environment variables
        // CASCETTE_REGION, CASCETTE_ENDPOINTS_RIBBIT, etc.
        builder = builder.add_source(
            Environment::with_prefix("CASCETTE")
                .separator("_")
                .try_parsing(true),
        );

        let config = builder.build().context("Failed to build configuration")?;

        let mut config: Self = config
            .try_deserialize()
            .context("Failed to deserialize configuration")?;

        // Migrate old config versions
        config.migrate()?;

        // Validate configuration
        config.validate()?;

        Ok(config)
    }

    /// Save configuration to default location
    pub fn save(&self) -> Result<()> {
        let config_path = crate::paths::config_file()?;
        self.save_to(config_path)
    }

    /// Save configuration to a specific file
    pub fn save_to(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        let path = path.as_ref();
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create config directory")?;
        }

        let toml = toml::to_string_pretty(self).context("Failed to serialize configuration")?;

        std::fs::write(path, toml)
            .with_context(|| format!("Failed to write configuration to {:?}", path))?;

        Ok(())
    }

    /// Expand region template in URL
    pub fn expand_url(&self, template: &str, region: Option<&str>) -> String {
        let region = region.unwrap_or(&self.region);
        #[allow(clippy::literal_string_with_formatting_args)]
        template.replace("{region}", region)
    }

    /// Get Ribbit host for a specific region
    pub fn ribbit_host(&self, region: Option<&str>) -> String {
        self.expand_url(&self.endpoints.ribbit_host, region)
    }

    /// Get TACT HTTP endpoint URL for a specific region
    pub fn tact_http_url(&self, region: Option<&str>) -> String {
        self.expand_url(&self.endpoints.tact_http, region)
    }

    /// Get TACT HTTPS endpoint URL for a specific region
    pub fn tact_https_url(&self, region: Option<&str>) -> String {
        self.expand_url(&self.endpoints.tact_https, region)
    }

    /// Get the Ribbit host as a full TCP URL (for display purposes)
    pub fn ribbit_url(&self, region: Option<&str>) -> String {
        format!("tcp://{}", self.ribbit_host(region))
    }

    /// Create default configuration file if it doesn't exist
    pub fn init() -> Result<()> {
        let config_path = crate::paths::config_file()?;

        if !config_path.exists() {
            let default_config = Self::default();
            default_config.save_to(config_path)?;
        }

        Ok(())
    }

    /// Get CDN hosts for a specific product
    /// Returns override hosts if configured, otherwise returns None
    pub fn get_cdn_hosts(&self, product: &str) -> Option<Vec<String>> {
        // First check product-specific overrides
        if let Some(product_hosts) = self.cdn_overrides.product_hosts.get(product) {
            if !product_hosts.is_empty() {
                return Some(product_hosts.clone());
            }
        }

        // Then check global CDN overrides
        if !self.cdn_overrides.hosts.is_empty() {
            return Some(self.cdn_overrides.hosts.clone());
        }

        // No overrides configured
        None
    }

    /// Convert to protocol `ClientConfig` with multiple URLs for redundancy
    ///
    /// This method creates multiple endpoint URLs for failover support.
    /// It uses the configured templates with common regions for redundancy.
    #[allow(clippy::similar_names)] // tact_http_urls and tact_https_urls are intentionally similar
    pub fn to_protocol_config(&self, region: Option<&str>) -> cascette_protocol::ClientConfig {
        let primary_region = region.unwrap_or(&self.region);

        // Expand templates with the primary region
        #[allow(clippy::literal_string_with_formatting_args)]
        let ribbit_url = format!(
            "tcp://{}",
            self.endpoints
                .ribbit_host
                .replace("{region}", primary_region)
        );

        #[allow(clippy::literal_string_with_formatting_args)]
        let tact_https_url = self
            .endpoints
            .tact_https
            .replace("{region}", primary_region);

        #[allow(clippy::literal_string_with_formatting_args)]
        let tact_http_url = self.endpoints.tact_http.replace("{region}", primary_region);

        cascette_protocol::ClientConfig {
            tact_https_url,
            tact_http_url,
            ribbit_url,
            cache_config: cascette_protocol::CacheConfig {
                cache_dir: self.cache.directory.clone(),
                memory_max_items: 10000,
                memory_max_size_bytes: 256 * 1024 * 1024, // 256MB
                disk_max_size_bytes: (self.cache.max_size_mb as usize) * 1024 * 1024,
                disk_max_file_size: 100 * 1024 * 1024, // 100MB per file
                ribbit_ttl: std::time::Duration::from_secs(self.cache.api_ttl_seconds),
                cdn_ttl: std::time::Duration::from_secs(self.cache.cdn_ttl_seconds),
                config_ttl: std::time::Duration::from_secs(self.cache.api_ttl_seconds),
            },
            connect_timeout: std::time::Duration::from_secs(self.network.connect_timeout),
            request_timeout: std::time::Duration::from_secs(self.network.request_timeout),
            retry_policy: cascette_protocol::RetryPolicy {
                max_attempts: self.network.retry_count,
                initial_backoff: std::time::Duration::from_millis(100),
                max_backoff: std::time::Duration::from_secs(10),
                multiplier: 2.0,
                jitter: true,
            },
        }
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<()> {
        // Validate region
        let valid_regions = ["us", "eu", "kr", "tw", "cn", "sg"];
        if !valid_regions.contains(&self.region.as_str()) {
            bail!(
                "Invalid region '{}'. Valid regions are: {}",
                self.region,
                valid_regions.join(", ")
            );
        }

        // Validate endpoint hosts/URLs
        if self.endpoints.ribbit_host.is_empty() {
            bail!("Ribbit host must be configured");
        }

        // Validate Ribbit host (should be host:port format)
        let host = &self.endpoints.ribbit_host;
        if !host.contains("{region}") && !host.contains(':') {
            bail!(
                "Ribbit host '{}' must be in host:port format or contain {{{{region}}}} placeholder",
                host
            );
        }
        // Should NOT have protocol prefix (just host:port)
        if host.starts_with("tcp://") || host.starts_with("http://") || host.starts_with("https://")
        {
            bail!(
                "Ribbit host '{}' should not include protocol scheme (use host:port format)",
                host
            );
        }

        // Validate TACT HTTP URL
        if !self.endpoints.tact_http.contains("{region}") {
            bail!("TACT HTTP endpoint must contain {{{{region}}}} placeholder");
        }
        if !self.endpoints.tact_http.starts_with("http://") {
            bail!("TACT HTTP endpoint must start with http://");
        }

        // Validate TACT HTTPS URL
        if !self.endpoints.tact_https.contains("{region}") {
            bail!("TACT HTTPS endpoint must contain {{{{region}}}} placeholder");
        }
        if !self.endpoints.tact_https.starts_with("https://") {
            bail!("TACT HTTPS endpoint must start with https://");
        }

        // Validate cache settings
        if self.cache.api_ttl_seconds == 0 {
            bail!("Cache API TTL must be greater than 0");
        }
        if self.cache.cdn_ttl_seconds == 0 {
            bail!("Cache CDN TTL must be greater than 0");
        }
        if self.cache.max_size_mb < 100 {
            bail!("Cache size must be at least 100 MB");
        }
        if self.cache.max_size_mb > 1024 * 100 {
            // 100GB
            bail!("Cache size cannot exceed 100 GB");
        }

        // Validate network settings
        if self.network.connect_timeout == 0 {
            bail!("Connect timeout must be greater than 0");
        }
        if self.network.request_timeout == 0 {
            bail!("Request timeout must be greater than 0");
        }
        if self.network.request_timeout <= self.network.connect_timeout {
            bail!("Request timeout must be greater than connect timeout");
        }
        if self.network.max_concurrent == 0 {
            bail!("Max concurrent downloads must be at least 1");
        }
        if self.network.max_concurrent > 32 {
            bail!("Max concurrent downloads cannot exceed 32");
        }

        Ok(())
    }

    /// Migrate configuration from older versions
    pub fn migrate(&mut self) -> Result<()> {
        if self.version < CONFIG_VERSION {
            // Migration logic for each version
            match self.version {
                0 => {
                    // Version 0 -> 1: Add version field, adjust cache size if needed
                    self.version = 1;

                    // If cache was set to old default of 1GB, update to new 10GB default
                    if self.cache.max_size_mb == 1024 {
                        self.cache.max_size_mb = 10240;
                    }
                }
                _ => {
                    // Unknown version, use defaults
                    *self = Self::default();
                }
            }

            // Save migrated configuration
            self.save()?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = CascetteConfig::default();
        assert_eq!(config.region, "us");
        assert!(!config.endpoints.ribbit_host.is_empty());
        assert_eq!(
            config.endpoints.ribbit_host,
            "{region}.version.battle.net:1119"
        );
    }

    #[test]
    fn test_url_expansion() {
        let config = CascetteConfig::default();

        // Test with default region
        assert_eq!(config.ribbit_url(None), "tcp://us.version.battle.net:1119");

        // Test with specific region
        assert_eq!(
            config.ribbit_url(Some("eu")),
            "tcp://eu.version.battle.net:1119"
        );
    }

    #[test]
    fn test_all_endpoints() {
        let config = CascetteConfig::default();

        assert_eq!(
            config.tact_http_url(Some("kr")),
            "http://kr.patch.battle.net:1119"
        );

        assert_eq!(
            config.tact_https_url(Some("cn")),
            "https://cn.version.battle.net"
        );
    }

    #[test]
    fn test_to_protocol_config() {
        let config = CascetteConfig::default();

        // Test conversion with default region (us)
        let protocol_config = config.to_protocol_config(None);

        // Should have single URLs with default region
        assert!(protocol_config.ribbit_url.starts_with("tcp://us."));
        assert_eq!(
            protocol_config.tact_https_url,
            "https://us.version.battle.net"
        );
        assert_eq!(
            protocol_config.tact_http_url,
            "http://us.patch.battle.net:1119"
        );

        // Test with specific region
        let protocol_config = config.to_protocol_config(Some("kr"));
        assert!(protocol_config.ribbit_url.starts_with("tcp://kr."));
        assert_eq!(
            protocol_config.tact_https_url,
            "https://kr.version.battle.net"
        );
        assert_eq!(
            protocol_config.tact_http_url,
            "http://kr.patch.battle.net:1119"
        );
    }
}
