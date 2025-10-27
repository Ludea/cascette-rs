//! Import provider trait and common functionality

use crate::error::ImportResult;
use crate::types::{BuildInfo, FileMapping, ProviderCapabilities};
use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;

/// Data source identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataSource {
    /// wago.tools build information
    Wago,
    /// `WoWDev` community listfile
    Listfile,
    /// `WoWDev` TACT keys repository
    TactKeys,
    /// Custom/third-party provider
    Custom(&'static str),
}

impl DataSource {
    /// Get display name for the data source
    pub fn display_name(self) -> &'static str {
        match self {
            DataSource::Wago => "wago.tools",
            DataSource::Listfile => "WoWDev Listfile",
            DataSource::TactKeys => "WoWDev TACT Keys",
            DataSource::Custom(name) => name,
        }
    }

    /// Get unique identifier for the data source
    pub fn id(self) -> &'static str {
        match self {
            DataSource::Wago => "wago",
            DataSource::Listfile => "listfile",
            DataSource::TactKeys => "tactkeys",
            DataSource::Custom(id) => id,
        }
    }
}

/// Provider information and metadata
#[derive(Debug, Clone)]
pub struct ImportProviderInfo {
    /// Data source type
    pub source: DataSource,

    /// Provider name
    pub name: String,

    /// Provider description
    pub description: String,

    /// Provider version
    pub version: String,

    /// Base URL or endpoint
    pub endpoint: Option<String>,

    /// Provider capabilities
    pub capabilities: ProviderCapabilities,

    /// Rate limit information (requests per minute)
    pub rate_limit: Option<u32>,

    /// Cache TTL (time to live)
    pub cache_ttl: Duration,
}

/// Trait for import data providers
#[async_trait]
pub trait ImportProvider: Send + Sync {
    /// Get provider information
    fn info(&self) -> &ImportProviderInfo;

    /// Initialize the provider (authenticate, validate config, etc.)
    async fn initialize(&mut self) -> ImportResult<()>;

    /// Check if the provider is available
    async fn is_available(&self) -> bool;

    /// Get build information for a product
    async fn get_builds(&self, product: &str) -> ImportResult<Vec<BuildInfo>>;

    /// Get all available builds (across all products)
    async fn get_all_builds(&self) -> ImportResult<Vec<BuildInfo>> {
        // Default implementation - providers can override for efficiency
        let mut all_builds = Vec::new();

        // This is a placeholder - real implementations would need to know
        // which products are available
        let products = vec!["wow", "diablo4", "overwatch2"];

        for product in products {
            match self.get_builds(product).await {
                Ok(mut builds) => all_builds.append(&mut builds),
                Err(_) => {} // Skip products that error
            }
        }

        Ok(all_builds)
    }

    /// Resolve file ID to path using community listfile
    async fn resolve_file_id(&self, file_id: u32) -> ImportResult<Option<String>> {
        // Default implementation returns None - only listfile providers implement this
        let _ = file_id;
        Ok(None)
    }

    /// Get file mappings for a range of file IDs
    async fn get_file_mappings(&self, file_ids: &[u32]) -> ImportResult<Vec<FileMapping>> {
        // Default implementation - providers can override for batch operations
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

    /// Search for builds matching criteria
    async fn search_builds(&self, criteria: &BuildSearchCriteria) -> ImportResult<Vec<BuildInfo>> {
        // Default implementation filters all builds - providers can optimize
        let all_builds = self.get_all_builds().await?;

        Ok(all_builds
            .into_iter()
            .filter(|build| criteria.matches(build))
            .collect())
    }

    /// Refresh cached data
    async fn refresh_cache(&self) -> ImportResult<()> {
        // Default implementation does nothing - providers can override
        Ok(())
    }

    /// Get cache statistics
    async fn cache_stats(&self) -> ImportResult<CacheStats> {
        // Default implementation returns empty stats
        Ok(CacheStats::default())
    }

    /// Get all file IDs available from this provider (if supported)
    /// Returns an empty vector if enumeration is not supported
    fn get_all_file_ids(&self) -> Vec<u32> {
        Vec::new()
    }
}

/// Build search criteria
#[derive(Debug, Clone, Default)]
pub struct BuildSearchCriteria {
    /// Product filter
    pub product: Option<String>,

    /// Version pattern (supports wildcards)
    pub version_pattern: Option<String>,

    /// Minimum build number
    pub min_build: Option<u32>,

    /// Maximum build number
    pub max_build: Option<u32>,

    /// Version type filter
    pub version_type: Option<String>,

    /// Region filter
    pub region: Option<String>,

    /// Date range filter
    pub date_range: Option<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>,

    /// Additional metadata filters
    pub metadata_filters: HashMap<String, String>,
}

impl BuildSearchCriteria {
    /// Check if a build matches these criteria
    pub fn matches(&self, build: &BuildInfo) -> bool {
        // Product filter
        if let Some(ref product) = self.product {
            if build.product != *product {
                return false;
            }
        }

        // Version pattern (basic wildcard support)
        if let Some(ref pattern) = self.version_pattern {
            if !matches_pattern(&build.version, pattern) {
                return false;
            }
        }

        // Build number range
        if let Some(min_build) = self.min_build {
            if build.build < min_build {
                return false;
            }
        }

        if let Some(max_build) = self.max_build {
            if build.build > max_build {
                return false;
            }
        }

        // Version type filter
        if let Some(ref version_type) = self.version_type {
            if build.version_type != *version_type {
                return false;
            }
        }

        // Region filter
        if let Some(ref region) = self.region {
            match &build.region {
                Some(build_region) => {
                    if build_region != region {
                        return false;
                    }
                }
                None => return false,
            }
        }

        // Date range filter
        if let Some((start, end)) = self.date_range {
            if let Some(timestamp) = build.timestamp {
                if timestamp < start || timestamp > end {
                    return false;
                }
            } else {
                return false;
            }
        }

        // Metadata filters
        for (key, expected_value) in &self.metadata_filters {
            if let Some(actual_value) = build.metadata.get(key) {
                if actual_value != expected_value {
                    return false;
                }
            } else {
                return false;
            }
        }

        true
    }
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of cached entries
    pub entries: usize,

    /// Cache hit count
    pub hits: u64,

    /// Cache miss count
    pub misses: u64,

    /// Last refresh timestamp
    pub last_refresh: Option<chrono::DateTime<chrono::Utc>>,

    /// Cache size in bytes
    pub size_bytes: usize,
}

impl CacheStats {
    /// Calculate cache hit rate as a percentage
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            (self.hits as f64 / total as f64) * 100.0
        }
    }
}

/// Basic pattern matching with wildcard support
fn matches_pattern(text: &str, pattern: &str) -> bool {
    // Simple wildcard matching - * matches any sequence of characters
    if pattern == "*" {
        return true;
    }

    if !pattern.contains('*') {
        return text == pattern;
    }

    // Split pattern by asterisks and check each part
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.is_empty() {
        return true;
    }

    let mut pos = 0;

    // Check first part (must be at the beginning)
    if !parts[0].is_empty() {
        if !text.starts_with(parts[0]) {
            return false;
        }
        pos += parts[0].len();
    }

    // Check middle parts
    for part in &parts[1..parts.len() - 1] {
        if part.is_empty() {
            continue;
        }

        if let Some(found_pos) = text[pos..].find(part) {
            pos += found_pos + part.len();
        } else {
            return false;
        }
    }

    // Check last part (must be at the end)
    if let Some(last_part) = parts.last() {
        if !last_part.is_empty() {
            return text[pos..].ends_with(last_part);
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_matching() {
        assert!(matches_pattern("1.15.2.55140", "*"));
        assert!(matches_pattern("1.15.2.55140", "1.15.*"));
        assert!(matches_pattern("1.15.2.55140", "*.55140"));
        assert!(matches_pattern("1.15.2.55140", "1.15.2.55140"));
        assert!(!matches_pattern("1.15.2.55140", "1.14.*"));
        assert!(!matches_pattern("1.15.2.55140", "*.55141"));
    }

    #[test]
    fn test_build_search_criteria() {
        let build = BuildInfo {
            product: "wow".to_string(),
            version: "1.15.2.55140".to_string(),
            build: 55140,
            version_type: "live".to_string(),
            region: Some("us".to_string()),
            timestamp: None,
            metadata: HashMap::new(),
        };

        let mut criteria = BuildSearchCriteria::default();
        assert!(criteria.matches(&build));

        criteria.product = Some("wow".to_string());
        assert!(criteria.matches(&build));

        criteria.product = Some("diablo4".to_string());
        assert!(!criteria.matches(&build));

        criteria.product = Some("wow".to_string());
        criteria.min_build = Some(55000);
        assert!(criteria.matches(&build));

        criteria.min_build = Some(60000);
        assert!(!criteria.matches(&build));
    }
}
