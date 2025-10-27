//! `WoWDev` community listfile integration for file ID to path mappings

use crate::error::{ImportError, ImportResult};
use crate::providers::{
    BuildSearchCriteria, CacheStats, DataSource, ImportProvider, ImportProviderInfo,
};
use crate::types::{BuildInfo, FileMapping, ProviderCapabilities};
use async_trait::async_trait;
use reqwest::Client;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::time::Duration;
use tracing::{debug, info, warn};

/// `WoWDev` listfile repository URL
const LISTFILE_REPO_URL: &str = "https://github.com/wowdev/wow-listfile";

/// Raw listfile download URL (using GitHub releases)
const LISTFILE_RAW_URL: &str =
    "https://github.com/wowdev/wow-listfile/releases/latest/download/community-listfile.csv";

/// `WoWDev` listfile provider for file ID mappings
pub struct ListfileProvider {
    info: ImportProviderInfo,
    client: Client,
    cache_dir: std::path::PathBuf,
    file_mappings: HashMap<u32, String>,
    last_refresh: Option<chrono::DateTime<chrono::Utc>>,
}

impl Default for ListfileProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ListfileProvider {
    /// Create a new listfile provider
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60)) // Listfile can be large
            .user_agent(format!("cascette-rs/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("Failed to create HTTP client");

        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("cascette-import")
            .join("listfile");

        Self {
            info: ImportProviderInfo {
                source: DataSource::Listfile,
                name: "WoWDev Listfile".to_string(),
                description: "Community-maintained file ID to path mappings".to_string(),
                version: "1.0.0".to_string(),
                endpoint: Some(LISTFILE_REPO_URL.to_string()),
                capabilities: ProviderCapabilities {
                    builds: false,
                    file_mappings: true,
                    real_time: false,
                    requires_auth: false,
                },
                rate_limit: Some(10), // Be conservative with GitHub
                cache_ttl: Duration::from_secs(86400), // 24 hours
            },
            client,
            cache_dir,
            file_mappings: HashMap::new(),
            last_refresh: None,
        }
    }

    /// Download and parse the community listfile
    async fn fetch_listfile(&self) -> ImportResult<HashMap<u32, String>> {
        debug!("Fetching community listfile from GitHub");

        let response = self
            .client
            .get(LISTFILE_RAW_URL)
            .send()
            .await
            .map_err(ImportError::Network)?;

        if !response.status().is_success() {
            return Err(ImportError::HttpStatus {
                provider: "listfile".to_string(),
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        let content = response.text().await.map_err(ImportError::Network)?;
        debug!("Downloaded listfile content: {} bytes", content.len());

        self.parse_listfile_content(&content)
    }
}

/// Check if a line should be skipped (empty or comment)
fn should_skip_line(line: &str) -> bool {
    line.trim().is_empty() || line.starts_with('#')
}

/// Parse and normalize a single CSV line (`file_id;path`)
fn parse_csv_line(line: &str, line_num: usize) -> Option<(u32, String)> {
    let (id_str, path) = line.split_once(';').or_else(|| {
        debug!("Invalid CSV format on line {}: {}", line_num, line);
        None
    })?;

    let file_id = id_str.trim().parse::<u32>().ok().or_else(|| {
        debug!("Failed to parse file ID from line {}: {}", line_num, line);
        None
    })?;

    let path = path.trim();
    let normalized_path = path.replace('\\', "/");

    Some((file_id, normalized_path))
}

impl ListfileProvider {
    /// Parse listfile CSV content
    fn parse_listfile_content(&self, content: &str) -> ImportResult<HashMap<u32, String>> {
        let mut mappings = HashMap::new();
        let reader = BufReader::new(content.as_bytes());

        let mut line_count = 0;
        let mut parsed_count = 0;

        for line in reader.lines() {
            line_count += 1;
            let line = line.map_err(ImportError::Io)?;

            if should_skip_line(&line) {
                continue;
            }

            if let Some((file_id, path)) = parse_csv_line(&line, line_count) {
                mappings.insert(file_id, path);
                parsed_count += 1;
            }
        }

        info!(
            "Parsed {} file mappings from {} lines",
            parsed_count, line_count
        );
        Ok(mappings)
    }

    /// Load mappings from disk cache
    async fn load_from_cache(&mut self) -> ImportResult<()> {
        let cache_file = self.cache_dir.join("mappings.json");

        if cache_file.exists() {
            debug!(
                "Loading file mappings from cache file: {}",
                cache_file.display()
            );

            let content = tokio::fs::read_to_string(&cache_file)
                .await
                .map_err(ImportError::Io)?;

            let cached_data: (HashMap<u32, String>, chrono::DateTime<chrono::Utc>) =
                serde_json::from_str(&content).map_err(ImportError::Json)?;

            // Check if cache is still valid (within TTL)
            let now = chrono::Utc::now();
            let cache_ttl = chrono::Duration::seconds(
                i64::try_from(self.info.cache_ttl.as_secs()).unwrap_or(i64::MAX),
            );

            if now - cached_data.1 < cache_ttl {
                self.file_mappings = cached_data.0;
                self.last_refresh = Some(cached_data.1);
                info!(
                    "Loaded {} file mappings from cache",
                    self.file_mappings.len()
                );
            } else {
                debug!("Listfile cache is stale");
            }
        } else {
            debug!("No listfile cache file found at {}", cache_file.display());
        }

        Ok(())
    }

    /// Save mappings to disk cache
    async fn save_to_cache(&self) -> ImportResult<()> {
        if !self.cache_dir.exists() {
            std::fs::create_dir_all(&self.cache_dir).map_err(ImportError::Io)?;
        }

        let cache_file = self.cache_dir.join("mappings.json");
        let now = chrono::Utc::now();

        // Create cache data with timestamp
        let cache_data = (self.file_mappings.clone(), now);
        let json = serde_json::to_string_pretty(&cache_data).map_err(ImportError::Json)?;

        tokio::fs::write(&cache_file, json)
            .await
            .map_err(ImportError::Io)?;

        debug!("Saved listfile cache to {}", cache_file.display());
        Ok(())
    }

    /// Refresh the file mappings cache
    async fn refresh_mappings_cache(&mut self) -> ImportResult<()> {
        debug!("Refreshing listfile mappings cache");

        let mappings = self.fetch_listfile().await?;
        self.file_mappings = mappings;
        self.last_refresh = Some(chrono::Utc::now());

        info!(
            "Cached {} file mappings from listfile",
            self.file_mappings.len()
        );
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
impl ImportProvider for ListfileProvider {
    fn info(&self) -> &ImportProviderInfo {
        &self.info
    }

    async fn initialize(&mut self) -> ImportResult<()> {
        info!("Initializing WoWDev listfile provider");

        // Test connection to GitHub
        match self.client.head(LISTFILE_REPO_URL).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    info!("GitHub repository is accessible");
                    Ok(())
                } else {
                    Err(ImportError::Provider {
                        provider: "listfile".to_string(),
                        message: format!("GitHub returned status: {}", response.status()),
                    })
                }
            }
            Err(e) => {
                warn!("GitHub repository is not accessible: {}", e);
                // Don't fail initialization - allow offline mode
                Ok(())
            }
        }
    }

    async fn is_available(&self) -> bool {
        match self.client.head(LISTFILE_REPO_URL).send().await {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }

    async fn get_builds(&self, _product: &str) -> ImportResult<Vec<BuildInfo>> {
        // Listfile provider doesn't provide build information
        Ok(Vec::new())
    }

    async fn resolve_file_id(&self, file_id: u32) -> ImportResult<Option<String>> {
        // Check cache first
        if let Some(path) = self.file_mappings.get(&file_id) {
            return Ok(Some(path.clone()));
        }

        // If cache is stale, indicate it needs refresh
        if self.needs_cache_refresh() {
            return Err(ImportError::Provider {
                provider: "listfile".to_string(),
                message: "File mapping cache needs refresh".to_string(),
            });
        }

        // File ID not found in current mappings
        Ok(None)
    }

    async fn get_file_mappings(&self, file_ids: &[u32]) -> ImportResult<Vec<FileMapping>> {
        let mut mappings = Vec::new();

        for &file_id in file_ids {
            if let Some(path) = self.resolve_file_id(file_id).await? {
                let filename = path.split('/').next_back().unwrap_or(&path).to_string();
                let directory = path.rsplit_once('/').map_or("", |x| x.0).to_string();

                mappings.push(FileMapping {
                    file_id,
                    path,
                    filename,
                    directory,
                });
            }
        }

        Ok(mappings)
    }

    async fn search_builds(&self, _criteria: &BuildSearchCriteria) -> ImportResult<Vec<BuildInfo>> {
        // Listfile provider doesn't provide build information
        Ok(Vec::new())
    }

    async fn refresh_cache(&self) -> ImportResult<()> {
        // This would need &mut self to modify the cache
        // For now, return success - the manager can handle cache refresh differently
        warn!("Cache refresh requested but not implemented in this context");
        Ok(())
    }

    async fn cache_stats(&self) -> ImportResult<CacheStats> {
        Ok(CacheStats {
            entries: self.file_mappings.len(),
            hits: 0,   // Would need to track in real implementation
            misses: 0, // Would need to track in real implementation
            last_refresh: self.last_refresh,
            size_bytes: self.file_mappings.len() * (std::mem::size_of::<u32>() + 50), // Rough estimate
        })
    }

    fn get_all_file_ids(&self) -> Vec<u32> {
        self.file_mappings.keys().copied().collect()
    }
}

/// Check if the listfile cache needs refresh
fn cache_needs_refresh(provider: &ListfileProvider) -> bool {
    if provider.file_mappings.is_empty() {
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

/// Attempt to refresh cache from GitHub and save to disk
async fn try_refresh_cache(provider: &mut ListfileProvider) {
    info!("Fetching listfile from GitHub");

    match provider.refresh_mappings_cache().await {
        Ok(()) => {
            // Save to disk after successful refresh
            if let Err(e) = provider.save_to_cache().await {
                warn!("Failed to save listfile cache to disk: {}", e);
            }
        }
        Err(e) => {
            warn!("Failed to refresh listfile cache: {}", e);
            // Continue anyway - provider can work with existing cache
        }
    }
}

/// Initialize provider, load cache, and refresh if needed
async fn initialize_listfile_provider(
    mut provider: ListfileProvider,
) -> ImportResult<ListfileProvider> {
    provider.initialize().await?;

    if let Err(e) = provider.load_from_cache().await {
        debug!("Failed to load listfile cache: {}", e);
    }

    if cache_needs_refresh(&provider) {
        if provider.is_available().await {
            try_refresh_cache(&mut provider).await;
        } else {
            warn!("GitHub not available and listfile cache is stale");
        }
    } else {
        debug!("Using existing listfile cache (still valid)");
    }

    Ok(provider)
}

/// Create a new listfile provider with cache preloaded
pub async fn create_listfile_provider() -> ImportResult<ListfileProvider> {
    let provider = ListfileProvider::new();
    let provider = initialize_listfile_provider(provider).await?;
    info!(
        "Listfile provider created with {} mappings",
        provider.file_mappings.len()
    );
    Ok(provider)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_listfile_parsing() {
        let provider = ListfileProvider::new();
        let content = r"# Comment line
123456;world/maps/azeroth/azeroth.wmo
789012;creature/human/male/humanmale.m2

345678;sound/music/zonemusic/stormwind.mp3
";

        let mappings = provider
            .parse_listfile_content(content)
            .expect("Failed to parse listfile content");

        assert_eq!(mappings.len(), 3);
        assert_eq!(
            mappings.get(&123_456),
            Some(&"world/maps/azeroth/azeroth.wmo".to_string())
        );
        assert_eq!(
            mappings.get(&789_012),
            Some(&"creature/human/male/humanmale.m2".to_string())
        );
        assert_eq!(
            mappings.get(&345_678),
            Some(&"sound/music/zonemusic/stormwind.mp3".to_string())
        );
    }

    #[test]
    fn test_path_normalization() {
        let provider = ListfileProvider::new();
        let content = "123456;world\\maps\\azeroth\\azeroth.wmo";

        let mappings = provider
            .parse_listfile_content(content)
            .expect("Failed to parse listfile content");

        // Backslashes should be converted to forward slashes
        assert_eq!(
            mappings.get(&123_456),
            Some(&"world/maps/azeroth/azeroth.wmo".to_string())
        );
    }

    #[tokio::test]
    async fn test_provider_creation() {
        let provider = ListfileProvider::new();
        assert_eq!(provider.info().source, DataSource::Listfile);
        assert!(!provider.info().capabilities.builds);
        assert!(provider.info().capabilities.file_mappings);
    }
}
