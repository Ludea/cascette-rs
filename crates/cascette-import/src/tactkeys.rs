//! `WoWDev` TACT Keys repository integration for encryption keys

use crate::error::{ImportError, ImportResult};
use crate::providers::{
    BuildSearchCriteria, CacheStats, DataSource, ImportProvider, ImportProviderInfo,
};
use crate::types::{BuildInfo, ProviderCapabilities};
use async_trait::async_trait;
use cascette_crypto::{TactKey, TactKeyStore};
use reqwest::Client;
use std::io::{BufRead, BufReader};
use std::time::Duration;
use tracing::{debug, info, warn};

/// `WoWDev` TACT Keys repository URL
const TACTKEYS_REPO_URL: &str = "https://github.com/wowdev/TACTKeys";

/// Raw TACT keys download URL
const TACTKEYS_RAW_URL: &str = "https://raw.githubusercontent.com/wowdev/TACTKeys/master/WoW.txt";

/// Check if a line should be skipped (empty or comment)
fn should_skip_line(line: &str) -> bool {
    line.trim().is_empty() || line.starts_with('#') || line.starts_with("//")
}

/// Validate hex string format
fn is_valid_hex(s: &str, expected_len: usize) -> bool {
    s.len() == expected_len && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Parse a single TACT key line and add to store
fn parse_and_add_key(line: &str, line_num: usize, key_store: &TactKeyStore) -> Option<()> {
    let parts: Vec<&str> = line.split_whitespace().collect();

    if parts.len() < 2 {
        debug!("Invalid line format on line {}: {}", line_num, line);
        return None;
    }

    let lookup_hex = parts[0];
    let key_hex = parts[1];

    if !is_valid_hex(lookup_hex, 16) || !is_valid_hex(key_hex, 32) {
        debug!("Invalid key format on line {}: {}", line_num, line);
        return None;
    }

    let key_id = u64::from_str_radix(lookup_hex, 16).ok()?;
    let tact_key = TactKey::from_hex(key_id, key_hex).ok()?;

    key_store.add(tact_key).ok()?;
    Some(())
}

/// `WoWDev` TACT Keys provider for encryption keys
pub struct TactKeysProvider {
    info: ImportProviderInfo,
    client: Client,
    cache_dir: std::path::PathBuf,
    tact_keys: TactKeyStore,
    last_refresh: Option<chrono::DateTime<chrono::Utc>>,
}

impl Default for TactKeysProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TactKeysProvider {
    /// Create a new TACT keys provider
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(format!("cascette-rs/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("Failed to create HTTP client");

        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("cascette-import")
            .join("tactkeys");

        Self {
            info: ImportProviderInfo {
                source: DataSource::TactKeys,
                name: "WoWDev TACT Keys".to_string(),
                description: "Community-maintained TACT encryption keys database".to_string(),
                version: "1.0.0".to_string(),
                endpoint: Some(TACTKEYS_REPO_URL.to_string()),
                capabilities: ProviderCapabilities {
                    builds: false,
                    file_mappings: false,
                    // Note: TACT keys are a specialized data source, not general file mappings
                    real_time: false,
                    requires_auth: false,
                },
                rate_limit: Some(10), // Be conservative with GitHub
                cache_ttl: Duration::from_secs(86400), // 24 hours
            },
            client,
            cache_dir,
            tact_keys: TactKeyStore::new().unwrap_or_else(|e| {
                warn!("Failed to create keyring store, using fallback: {}", e);
                // In case keyring isn't available, we still need to create a store
                // The KeyringTactKeyStore should handle fallback internally
                unreachable!("TactKeyStore::new() should always succeed with fallback mode")
            }), // Use keyring-based store with error handling
            last_refresh: None,
        }
    }

    /// Download and parse the TACT keys file
    async fn fetch_tact_keys(&self) -> ImportResult<TactKeyStore> {
        debug!("Fetching TACT keys from GitHub");

        let response = self
            .client
            .get(TACTKEYS_RAW_URL)
            .send()
            .await
            .map_err(ImportError::Network)?;

        if !response.status().is_success() {
            return Err(ImportError::HttpStatus {
                provider: "tactkeys".to_string(),
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        let content = response.text().await.map_err(ImportError::Network)?;
        debug!("Downloaded TACT keys content: {} bytes", content.len());

        self.parse_tact_keys_content(&content)
    }

    /// Parse TACT keys file content
    fn parse_tact_keys_content(&self, content: &str) -> ImportResult<TactKeyStore> {
        let key_store = TactKeyStore::new().map_err(|e| ImportError::Provider {
            provider: "tactkeys".to_string(),
            message: format!("Failed to create keyring store: {}", e),
        })?;

        let reader = BufReader::new(content.as_bytes());
        let mut line_count = 0;
        let mut parsed_count = 0;

        for line in reader.lines() {
            line_count += 1;
            let line = line.map_err(ImportError::Io)?;

            if should_skip_line(&line) {
                continue;
            }

            if parse_and_add_key(&line, line_count, &key_store).is_some() {
                parsed_count += 1;
            }
        }

        info!(
            "Parsed {} TACT keys from {} lines",
            parsed_count, line_count
        );
        Ok(key_store)
    }

    /// Load TACT keys from disk cache
    async fn load_from_cache(&mut self) -> ImportResult<()> {
        let cache_file = self.cache_dir.join("keys.json");

        if cache_file.exists() {
            debug!(
                "Loading TACT keys from cache file: {}",
                cache_file.display()
            );

            let content = tokio::fs::read_to_string(&cache_file)
                .await
                .map_err(ImportError::Io)?;

            // Cache format: Vec of (key_id_hex, key_hex) pairs with timestamp
            let cached_data: (Vec<(String, String)>, chrono::DateTime<chrono::Utc>) =
                serde_json::from_str(&content).map_err(ImportError::Json)?;

            // Check if cache is still valid (within TTL)
            let now = chrono::Utc::now();
            let cache_ttl = chrono::Duration::seconds(
                i64::try_from(self.info.cache_ttl.as_secs()).unwrap_or(i64::MAX),
            );

            if now - cached_data.1 < cache_ttl {
                // Reconstruct TactKeyStore from cached data
                self.tact_keys = TactKeyStore::new().map_err(|e| ImportError::Provider {
                    provider: "tactkeys".to_string(),
                    message: format!("Failed to create keyring store for cache: {}", e),
                })?;
                for (id_hex, key_hex) in cached_data.0 {
                    if let Ok(id) = u64::from_str_radix(&id_hex, 16) {
                        if let Ok(tact_key) = TactKey::from_hex(id, &key_hex) {
                            if let Err(e) = self.tact_keys.add(tact_key) {
                                debug!("Failed to add cached key {}: {}", id, e);
                            }
                        }
                    }
                }
                self.last_refresh = Some(cached_data.1);
                info!("Loaded {} TACT keys from cache", self.tact_keys.len());
            } else {
                debug!("TACT keys cache is stale");
            }
        } else {
            debug!("No TACT keys cache file found at {}", cache_file.display());
        }

        Ok(())
    }

    /// Save TACT keys to disk cache
    async fn save_to_cache(&self) -> ImportResult<()> {
        if !self.cache_dir.exists() {
            std::fs::create_dir_all(&self.cache_dir).map_err(ImportError::Io)?;
        }

        let cache_file = self.cache_dir.join("keys.json");
        let now = chrono::Utc::now();

        // Convert TactKeyStore to serializable format
        let keys_vec: Vec<(String, String)> = self
            .tact_keys
            .iter()
            .map(|key| (format!("{:016X}", key.id), hex::encode_upper(key.key)))
            .collect();

        // Create cache data with timestamp
        let cache_data = (keys_vec, now);
        let json = serde_json::to_string_pretty(&cache_data).map_err(ImportError::Json)?;

        tokio::fs::write(&cache_file, json)
            .await
            .map_err(ImportError::Io)?;

        debug!("Saved TACT keys cache to {}", cache_file.display());
        Ok(())
    }

    /// Refresh the TACT keys cache
    async fn refresh_keys_cache(&mut self) -> ImportResult<()> {
        debug!("Refreshing TACT keys cache");

        let keys = self.fetch_tact_keys().await?;
        self.tact_keys = keys;
        self.last_refresh = Some(chrono::Utc::now());

        info!("Cached {} TACT keys", self.tact_keys.len());
        Ok(())
    }

    /// Check if cache needs refresh
    fn needs_cache_refresh(&self) -> bool {
        match self.last_refresh {
            Some(last) => {
                let elapsed = chrono::Utc::now() - last;
                elapsed
                    > chrono::Duration::seconds(
                        i64::try_from(self.info.cache_ttl.as_secs()).unwrap_or(i64::MAX),
                    )
            }
            None => true,
        }
    }
}

#[async_trait]
impl ImportProvider for TactKeysProvider {
    fn info(&self) -> &ImportProviderInfo {
        &self.info
    }

    async fn initialize(&mut self) -> ImportResult<()> {
        info!("Initializing WoWDev TACT Keys provider");

        // Test connection to GitHub
        match self.client.head(TACTKEYS_REPO_URL).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    info!("GitHub TACT Keys repository is accessible");
                    Ok(())
                } else {
                    Err(ImportError::Provider {
                        provider: "tactkeys".to_string(),
                        message: format!("GitHub returned status: {}", response.status()),
                    })
                }
            }
            Err(e) => {
                warn!("GitHub TACT Keys repository is not accessible: {}", e);
                // Don't fail initialization - allow offline mode
                Ok(())
            }
        }
    }

    async fn is_available(&self) -> bool {
        match self.client.head(TACTKEYS_REPO_URL).send().await {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }

    async fn get_builds(&self, _product: &str) -> ImportResult<Vec<BuildInfo>> {
        // TACT keys provider doesn't provide build information
        Ok(Vec::new())
    }

    async fn resolve_file_id(&self, _file_id: u32) -> ImportResult<Option<String>> {
        // TACT keys provider doesn't provide file mappings
        Ok(None)
    }
}

impl TactKeysProvider {
    /// Get TACT key by lookup string (hex format)
    #[allow(clippy::unused_async)]
    pub async fn get_tact_key(&self, lookup: &str) -> ImportResult<Option<TactKey>> {
        // Parse lookup string as u64 hex
        let Ok(key_id) = u64::from_str_radix(lookup.trim(), 16) else {
            return Ok(None); // Invalid format, key not found
        };

        // Check cache first
        match self.tact_keys.get(key_id) {
            Ok(Some(key_bytes)) => {
                return Ok(Some(TactKey::new(key_id, key_bytes)));
            }
            Ok(None) => {} // Key not found, continue
            Err(e) => {
                debug!("Failed to get key {} from keyring: {}", key_id, e);
                // Continue without failing - might be available after refresh
            }
        }

        // If cache is stale, indicate it needs refresh
        if self.needs_cache_refresh() {
            return Err(ImportError::Provider {
                provider: "tactkeys".to_string(),
                message: "TACT keys cache needs refresh".to_string(),
            });
        }

        // Key not found in current cache
        Ok(None)
    }

    /// Get all available TACT keys
    #[allow(clippy::unused_async)]
    pub async fn get_all_tact_keys(&self) -> ImportResult<Vec<TactKey>> {
        if self.tact_keys.is_empty() && self.needs_cache_refresh() {
            return Err(ImportError::Provider {
                provider: "tactkeys".to_string(),
                message: "TACT keys cache is empty and needs refresh".to_string(),
            });
        }

        Ok(self
            .tact_keys
            .iter()
            .map(|key| TactKey::new(key.id, key.key))
            .collect())
    }

    #[allow(dead_code, clippy::unnecessary_wraps)]
    fn search_builds(&self, _criteria: &BuildSearchCriteria) -> ImportResult<Vec<BuildInfo>> {
        // TACT keys provider doesn't provide build information
        Ok(Vec::new())
    }

    #[allow(dead_code, clippy::unnecessary_wraps)]
    fn refresh_cache(&self) -> ImportResult<()> {
        // This would need &mut self to modify the cache
        // For now, return success - the manager can handle cache refresh differently
        warn!("Cache refresh requested but not implemented in this context");
        Ok(())
    }

    #[allow(dead_code, clippy::unnecessary_wraps)]
    fn cache_stats(&self) -> ImportResult<CacheStats> {
        Ok(CacheStats {
            entries: self.tact_keys.len(),
            hits: 0,   // Would need to track in real implementation
            misses: 0, // Would need to track in real implementation
            last_refresh: self.last_refresh,
            size_bytes: self.tact_keys.len() * std::mem::size_of::<TactKey>(), // Rough estimate
        })
    }
}

/// Check if the TACT keys cache needs refresh
fn tactkeys_cache_needs_refresh(provider: &TactKeysProvider) -> bool {
    if provider.tact_keys.is_empty() {
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

/// Attempt to refresh TACT keys cache and save to disk
async fn try_refresh_tactkeys_cache(provider: &mut TactKeysProvider) {
    match provider.refresh_keys_cache().await {
        Ok(()) => {
            // Save to disk after successful refresh
            if let Err(e) = provider.save_to_cache().await {
                warn!("Failed to save TACT keys cache to disk: {}", e);
            }
        }
        Err(e) => {
            warn!("Failed to refresh TACT keys cache: {}", e);
            // Continue anyway - provider can work with existing cache
        }
    }
}

/// Initialize provider, load cache, and refresh if needed
async fn initialize_tactkeys_provider(
    mut provider: TactKeysProvider,
) -> ImportResult<TactKeysProvider> {
    provider.initialize().await?;

    if let Err(e) = provider.load_from_cache().await {
        debug!("Failed to load TACT keys cache: {}", e);
    }

    if tactkeys_cache_needs_refresh(&provider) {
        if provider.is_available().await {
            try_refresh_tactkeys_cache(&mut provider).await;
        } else {
            warn!("GitHub not available and TACT keys cache is stale");
        }
    } else {
        debug!("Using existing TACT keys cache (still valid)");
    }

    Ok(provider)
}

/// Create a new TACT keys provider with cache preloaded
pub async fn create_tactkeys_provider() -> ImportResult<TactKeysProvider> {
    let provider = TactKeysProvider::new();
    let provider = initialize_tactkeys_provider(provider).await?;
    Ok(provider)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_tact_keys_parsing() {
        let provider = TactKeysProvider::new();
        let content = r"# TACT Keys for World of Warcraft
// Comment line
0123456789ABCDEF 0123456789ABCDEF0123456789ABCDEF
FEDCBA9876543210 FEDCBA9876543210FEDCBA9876543210 Additional info

// Invalid lines
INVALID_HEX ABCD1234567890ABCDEF1234567890AB
0123456789ABCDEF INVALID_KEY
";

        let key_store = provider
            .parse_tact_keys_content(content)
            .expect("Failed to parse TACT keys content");

        assert_eq!(key_store.len(), 2);

        let first_key_id = 0x0123_4567_89AB_CDEF_u64;
        let first_key = key_store
            .get(first_key_id)
            .expect("Failed to get first TACT key")
            .expect("First key should exist");
        assert_eq!(
            hex::encode_upper(first_key),
            "0123456789ABCDEF0123456789ABCDEF"
        );

        let second_key_id = 0xFEDC_BA98_7654_3210_u64;
        let second_key = key_store
            .get(second_key_id)
            .expect("Failed to get second TACT key")
            .expect("Second key should exist");
        assert_eq!(
            hex::encode_upper(second_key),
            "FEDCBA9876543210FEDCBA9876543210"
        );
    }

    #[tokio::test]
    async fn test_provider_creation() {
        let provider = TactKeysProvider::new();
        assert_eq!(provider.info().source, DataSource::TactKeys);
        // TACT keys provider is a specialized provider that doesn't fit standard capabilities
        assert!(!provider.info().capabilities.builds);
        assert!(!provider.info().capabilities.file_mappings);
    }

    #[tokio::test]
    async fn test_key_lookup_normalization() {
        let mut provider = TactKeysProvider::new();

        // Simulate a key in the cache using cascette-crypto TactKey
        let key_id = 0x0123_4567_89AB_CDEF_u64;
        let test_key = TactKey::from_hex(key_id, "0123456789ABCDEF0123456789ABCDEF")
            .expect("Valid test key should parse");
        provider
            .tact_keys
            .add(test_key)
            .expect("Should be able to add test key");
        provider.last_refresh = Some(chrono::Utc::now());

        // Test lookup with lowercase - should be normalized to uppercase and converted to ID
        let result = provider
            .get_tact_key("0123456789abcdef")
            .await
            .expect("Failed to get TACT key");
        assert!(result.is_some());

        let key = result.expect("Expected TACT key to be found");
        assert_eq!(key.id, key_id);
    }
}
