//! Central metadata orchestration for NGDP/CASC operations
//!
//! This module provides the high-level orchestrator that coordinates FileDataID
//! resolution and TACT key management for both client and server operations.

use crate::error::MetadataResult;
use crate::fdid::{FileDataIdMapping, FileDataIdProvider, FileDataIdService, FileDataIdStats};
use crate::tact::{TactKeyManager, TactKeyStats};
use serde::Serialize;

/// Configuration for the metadata orchestrator
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    /// Data directory for storing keys and caches
    pub data_dir: std::path::PathBuf,
    /// Whether to enable performance metrics collection
    pub enable_metrics: bool,
    /// Maximum memory usage for caching (bytes)
    pub max_cache_memory: usize,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            data_dir: std::env::temp_dir().join("cascette-metadata"),
            enable_metrics: true,
            max_cache_memory: 256 * 1024 * 1024, // 256 MB
        }
    }
}

/// Represents information about content in the CASC system
#[derive(Debug, Clone)]
pub struct ContentInfo {
    /// The FileDataID mapping
    pub mapping: FileDataIdMapping,
    /// Whether this content requires encryption
    pub requires_encryption: bool,
    /// Recommended compression level (0-9)
    pub compression_level: u8,
    /// Content category for organization
    pub category: ContentCategory,
}

/// Categories of content for organization and policy decisions
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentCategory {
    /// Game executable files
    Executable,
    /// Audio files (music, sound effects)
    Audio,
    /// Texture and model files
    Graphics,
    /// User interface files
    Interface,
    /// Game data and scripts
    Data,
    /// Unknown or miscellaneous files
    Unknown,
}

impl ContentInfo {
    /// Create new content info from a mapping
    pub fn new(mapping: FileDataIdMapping) -> Self {
        let category = Self::categorize_from_path(&mapping.path);
        let (requires_encryption, compression_level) = Self::get_defaults_for_category(&category);

        Self {
            mapping,
            requires_encryption,
            compression_level,
            category,
        }
    }

    /// Categorize content based on file path
    fn categorize_from_path(file_path: &str) -> ContentCategory {
        let path_lower = file_path.to_lowercase();

        // Check file extensions first (more specific)
        if path_lower.ends_with(".exe") || path_lower.ends_with(".dll") {
            ContentCategory::Executable
        } else if path_lower.ends_with(".lua")
            || path_lower.ends_with(".toc")
            || path_lower.ends_with(".xml")
        {
            ContentCategory::Data
        } else if path_lower.ends_with(".ogg")
            || path_lower.ends_with(".wav")
            || path_lower.ends_with(".mp3")
        {
            ContentCategory::Audio
        } else if path_lower.ends_with(".blp")
            || path_lower.ends_with(".m2")
            || path_lower.ends_with(".wmo")
        {
            ContentCategory::Graphics
        } else if path_lower.contains("interface/") {
            // Only categorize as Interface if no more specific file type found
            ContentCategory::Interface
        } else {
            ContentCategory::Unknown
        }
    }

    /// Get default policies for a content category
    fn get_defaults_for_category(category: &ContentCategory) -> (bool, u8) {
        match category {
            ContentCategory::Executable => (true, 9), // High security, max compression
            ContentCategory::Audio => (false, 6),     // No encryption, medium compression
            ContentCategory::Graphics => (false, 7),  // No encryption, high compression
            ContentCategory::Interface => (false, 5), // No encryption, medium compression
            ContentCategory::Data => (false, 4),      // No encryption, low compression
            ContentCategory::Unknown => (false, 3),   // Conservative defaults
        }
    }
}

/// Central orchestrator for NGDP/CASC metadata operations
///
/// The `MetadataOrchestrator` coordinates FileDataID resolution and TACT key
/// management, providing a unified interface for content metadata operations.
pub struct MetadataOrchestrator {
    /// FileDataID service for path resolution
    fdid_service: FileDataIdService,
    /// TACT key manager for encryption
    tact_manager: TactKeyManager,
    /// Configuration settings
    config: OrchestratorConfig,
    /// Content validation cache for performance
    validation_cache: std::collections::HashMap<u32, ValidationResult>,
}

impl MetadataOrchestrator {
    /// Create a new metadata orchestrator
    ///
    /// This sets up the required services with the given configuration.
    pub fn new(
        fdid_provider: Box<dyn FileDataIdProvider>,
        config: OrchestratorConfig,
    ) -> MetadataResult<Self> {
        // Create TACT key manager
        let tact_manager = TactKeyManager::new(&config.data_dir)?;

        // Create FileDataID service
        let fdid_service = FileDataIdService::new(fdid_provider);

        Ok(Self {
            fdid_service,
            tact_manager,
            config,
            validation_cache: std::collections::HashMap::new(),
        })
    }

    /// Create a new orchestrator with default configuration
    pub fn with_defaults(fdid_provider: Box<dyn FileDataIdProvider>) -> MetadataResult<Self> {
        Self::new(fdid_provider, OrchestratorConfig::default())
    }

    /// Load FileDataID mappings from the configured provider
    pub async fn load_mappings(&mut self) -> MetadataResult<()> {
        self.fdid_service.load_from_provider().await
    }

    /// Resolve a FileDataID to file path (synchronous, from cache only)
    pub fn resolve_file_path(&self, file_data_id: u32) -> MetadataResult<Option<String>> {
        self.fdid_service.get_file_path(file_data_id)
    }

    /// Resolve a FileDataID to file path with lazy loading
    pub async fn resolve_file_path_async(
        &mut self,
        file_data_id: u32,
    ) -> MetadataResult<Option<String>> {
        self.fdid_service.get_file_path_async(file_data_id).await
    }

    /// Resolve a file path to FileDataID
    pub fn resolve_file_data_id(&self, file_path: &str) -> MetadataResult<Option<u32>> {
        self.fdid_service.get_file_data_id(file_path)
    }

    /// Get content information for a FileDataID
    pub fn get_content_info(&self, file_data_id: u32) -> MetadataResult<Option<ContentInfo>> {
        if let Some(file_path) = self.resolve_file_path(file_data_id)? {
            let mapping = FileDataIdMapping::new(file_data_id, file_path);
            Ok(Some(ContentInfo::new(mapping)))
        } else {
            Ok(None)
        }
    }

    /// Get content information for a file path
    pub fn get_content_info_by_path(&self, file_path: &str) -> MetadataResult<Option<ContentInfo>> {
        if let Some(file_data_id) = self.resolve_file_data_id(file_path)? {
            let mapping = FileDataIdMapping::new(file_data_id, file_path.to_string());
            Ok(Some(ContentInfo::new(mapping)))
        } else {
            Ok(None)
        }
    }

    /// Add a TACT encryption key
    pub fn add_tact_key(
        &mut self,
        key_id: u64,
        key_hex: &str,
        source: &str,
        description: Option<String>,
        product: Option<String>,
        build: Option<u32>,
    ) -> MetadataResult<()> {
        self.tact_manager
            .add_key(key_id, key_hex, source, description, product, build)
    }

    /// Get a TACT key by ID
    pub fn get_tact_key(
        &mut self,
        key_id: u64,
    ) -> MetadataResult<Option<cascette_crypto::TactKey>> {
        if let Some((key, _metadata)) = self.tact_manager.get_key(key_id)? {
            Ok(Some(key))
        } else {
            Ok(None)
        }
    }

    /// Get statistics about the orchestrator
    pub fn get_stats(&mut self) -> MetadataResult<OrchestratorStats> {
        Ok(OrchestratorStats {
            fdid_stats: self.fdid_service.get_stats()?,
            tact_stats: self.tact_manager.get_stats(),
        })
    }

    /// Get the orchestrator configuration
    pub fn get_config(&self) -> &OrchestratorConfig {
        &self.config
    }

    /// Check if a FileDataID exists
    pub fn has_file_data_id(&self, file_data_id: u32) -> MetadataResult<bool> {
        self.fdid_service.has_file_data_id(file_data_id)
    }

    /// Check if a file path exists
    pub fn has_file_path(&self, file_path: &str) -> MetadataResult<bool> {
        self.fdid_service.has_file_path(file_path)
    }

    /// Get the total number of FileDataID mappings
    pub fn mapping_count(&self) -> MetadataResult<usize> {
        self.fdid_service.mapping_count()
    }

    /// Export all FileDataID mappings
    pub fn export_mappings(&self) -> MetadataResult<Vec<FileDataIdMapping>> {
        self.fdid_service.export_mappings()
    }

    /// Resolve content with full metadata including encryption status
    ///
    /// This method combines FileDataID resolution with TACT key information
    /// to provide comprehensive content metadata.
    pub fn resolve_content_with_encryption(
        &mut self,
        file_data_id: u32,
    ) -> MetadataResult<Option<FullContentInfo>> {
        if let Some(content_info) = self.get_content_info(file_data_id)? {
            let encryption_info = if content_info.requires_encryption {
                // For demonstration, we'll check for common TACT key patterns
                // In practice, this would use actual encryption key mappings
                self.find_encryption_key_for_content(&content_info)
            } else {
                None
            };

            Ok(Some(FullContentInfo {
                content: content_info,
                encryption_key_id: encryption_info.map(|k| k.0),
                encryption_available: encryption_info.is_some(),
            }))
        } else {
            Ok(None)
        }
    }

    /// Bulk resolve multiple FileDataIDs with content information
    pub fn bulk_resolve_content(
        &mut self,
        file_data_ids: &[u32],
    ) -> MetadataResult<Vec<(u32, Option<ContentInfo>)>> {
        let mut results = Vec::with_capacity(file_data_ids.len());

        for &file_data_id in file_data_ids {
            let content_info = self.get_content_info(file_data_id)?;
            results.push((file_data_id, content_info));
        }

        Ok(results)
    }

    /// Validate content integrity combining FileDataID and encryption checks
    pub fn validate_content(&mut self, file_data_id: u32) -> MetadataResult<ValidationResult> {
        // Check cache first
        if let Some(cached_result) = self.validation_cache.get(&file_data_id) {
            return Ok(cached_result.clone());
        }

        let result = if let Some(content_info) = self.get_content_info(file_data_id)? {
            let mut issues = Vec::new();
            let mut warnings = Vec::new();

            // Check if encryption is required but key is missing
            if content_info.requires_encryption
                && self
                    .find_encryption_key_for_content(&content_info)
                    .is_none()
            {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Error,
                    message: format!(
                        "Content requires encryption but no TACT key available for {}",
                        content_info.mapping.path
                    ),
                    category: ValidationCategory::Encryption,
                });
            }

            // Check for suspicious file patterns
            if content_info.mapping.path.to_lowercase().contains(".exe")
                && !content_info.requires_encryption
            {
                warnings.push(ValidationIssue {
                    severity: ValidationSeverity::Warning,
                    message: format!(
                        "Executable file {} is not encrypted",
                        content_info.mapping.path
                    ),
                    category: ValidationCategory::Security,
                });
            }

            ValidationResult {
                file_data_id,
                is_valid: issues.is_empty(),
                issues,
                warnings,
                validated_at: chrono::Utc::now(),
            }
        } else {
            ValidationResult {
                file_data_id,
                is_valid: false,
                issues: vec![ValidationIssue {
                    severity: ValidationSeverity::Error,
                    message: format!("FileDataID {} not found in mappings", file_data_id),
                    category: ValidationCategory::Mapping,
                }],
                warnings: Vec::new(),
                validated_at: chrono::Utc::now(),
            }
        };

        // Cache the result
        self.validation_cache.insert(file_data_id, result.clone());
        Ok(result)
    }

    /// Clear the validation cache
    pub fn clear_validation_cache(&mut self) {
        self.validation_cache.clear();
    }

    /// Get validation cache statistics
    pub fn get_validation_cache_stats(&self) -> ValidationCacheStats {
        ValidationCacheStats {
            cached_validations: self.validation_cache.len(),
            cache_memory_usage: self.validation_cache.len()
                * std::mem::size_of::<ValidationResult>(),
        }
    }

    /// Find potential encryption key for content (helper method)
    fn find_encryption_key_for_content(
        &mut self,
        content_info: &ContentInfo,
    ) -> Option<(u64, cascette_crypto::TactKey)> {
        // This is a simplified example - in practice, you'd have specific
        // mappings between content and encryption keys
        let keys = self.tact_manager.list_keys(None, None, false);

        // For demonstration, return the first available key for encrypted content
        // In practice, this would use content-specific key mappings
        if !keys.is_empty() && content_info.requires_encryption {
            let (key, _metadata) = &keys[0];
            Some((key.id, *key))
        } else {
            None
        }
    }

    /// Create a builder for complex orchestrator operations
    pub fn builder() -> OrchestratorBuilder {
        OrchestratorBuilder::new()
    }

    /// Get comprehensive health status of the orchestrator
    pub fn get_health_status(&mut self) -> HealthStatus {
        let stats = self.get_stats().unwrap_or_else(|_| OrchestratorStats {
            fdid_stats: FileDataIdStats::default(),
            tact_stats: TactKeyStats::default(),
        });
        let mut issues = Vec::new();
        let mut warnings = Vec::new();

        // Check for critical issues
        if stats.fdid_stats.total_mappings == 0 {
            issues.push("No FileDataID mappings loaded".to_string());
        }

        if stats.tact_stats.total_keys == 0 {
            warnings.push("No TACT keys available".to_string());
        }

        // Check cache effectiveness
        if let Some(cache_stats) = &stats.fdid_stats.cache_stats {
            if cache_stats.hit_rate < 50.0 {
                warnings.push("Low FileDataID cache hit rate".to_string());
            }
        }

        HealthStatus {
            is_healthy: issues.is_empty(),
            issues,
            warnings,
            last_check: chrono::Utc::now(),
            service_stats: stats,
        }
    }
}

/// Extended content information including encryption details
#[derive(Debug, Clone)]
pub struct FullContentInfo {
    /// Basic content information
    pub content: ContentInfo,
    /// TACT key ID if encryption is available
    pub encryption_key_id: Option<u64>,
    /// Whether encryption key is available
    pub encryption_available: bool,
}

/// Result of content validation
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// The FileDataID that was validated
    pub file_data_id: u32,
    /// Whether the content passed all validation checks
    pub is_valid: bool,
    /// Critical issues that prevent content use
    pub issues: Vec<ValidationIssue>,
    /// Non-critical warnings
    pub warnings: Vec<ValidationIssue>,
    /// When this validation was performed
    pub validated_at: chrono::DateTime<chrono::Utc>,
}

/// Individual validation issue
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    /// Severity level of the issue
    pub severity: ValidationSeverity,
    /// Human-readable description
    pub message: String,
    /// Category of the validation issue
    pub category: ValidationCategory,
}

/// Severity levels for validation issues
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationSeverity {
    /// Critical error that prevents use
    Error,
    /// Warning that should be addressed
    Warning,
    /// Informational notice
    Info,
}

/// Categories of validation issues
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationCategory {
    /// Issues with FileDataID mappings
    Mapping,
    /// Issues with encryption/keys
    Encryption,
    /// Security-related concerns
    Security,
    /// Performance-related issues
    Performance,
}

/// Validation cache statistics
#[derive(Debug, Clone)]
pub struct ValidationCacheStats {
    /// Number of cached validation results
    pub cached_validations: usize,
    /// Approximate memory usage of cache
    pub cache_memory_usage: usize,
}

/// Overall health status of the orchestrator
#[derive(Debug, Clone)]
pub struct HealthStatus {
    /// Whether the orchestrator is in a healthy state
    pub is_healthy: bool,
    /// Critical issues requiring attention
    pub issues: Vec<String>,
    /// Non-critical warnings
    pub warnings: Vec<String>,
    /// When this health check was performed
    pub last_check: chrono::DateTime<chrono::Utc>,
    /// Current service statistics
    pub service_stats: OrchestratorStats,
}

/// Statistics about the orchestrator
#[derive(Debug, Clone, Serialize)]
pub struct OrchestratorStats {
    /// FileDataID service statistics
    pub fdid_stats: FileDataIdStats,
    /// TACT key manager statistics
    pub tact_stats: TactKeyStats,
}

/// Builder for complex orchestrator operations
pub struct OrchestratorBuilder {
    data_dir: Option<std::path::PathBuf>,
    enable_metrics: bool,
    max_cache_memory: usize,
    fdid_provider: Option<Box<dyn FileDataIdProvider>>,
}

impl OrchestratorBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            data_dir: None,
            enable_metrics: true,
            max_cache_memory: 256 * 1024 * 1024,
            fdid_provider: None,
        }
    }

    /// Set the data directory
    #[must_use]
    pub fn data_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.data_dir = Some(dir);
        self
    }

    /// Enable or disable metrics collection
    #[must_use]
    pub fn enable_metrics(mut self, enable: bool) -> Self {
        self.enable_metrics = enable;
        self
    }

    /// Set maximum cache memory usage
    #[must_use]
    pub fn max_cache_memory(mut self, bytes: usize) -> Self {
        self.max_cache_memory = bytes;
        self
    }

    /// Set the FileDataID provider
    #[must_use]
    pub fn fdid_provider(mut self, provider: Box<dyn FileDataIdProvider>) -> Self {
        self.fdid_provider = Some(provider);
        self
    }

    /// Build the orchestrator
    pub fn build(self) -> MetadataResult<MetadataOrchestrator> {
        let data_dir = self
            .data_dir
            .unwrap_or_else(|| std::env::temp_dir().join("cascette-metadata"));
        let fdid_provider = self.fdid_provider.ok_or_else(|| {
            crate::error::MetadataError::InvalidConfiguration(
                "FileDataID provider is required".to_string(),
            )
        })?;

        let config = OrchestratorConfig {
            data_dir,
            enable_metrics: self.enable_metrics,
            max_cache_memory: self.max_cache_memory,
        };

        MetadataOrchestrator::new(fdid_provider, config)
    }
}

impl Default for OrchestratorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::fdid::MemoryProvider;
    use tempfile::TempDir;

    #[test]
    fn test_content_categorization() {
        let mapping =
            FileDataIdMapping::new(12345, "Interface/AddOns/MyAddon/MyAddon.toc".to_string());
        let content = ContentInfo::new(mapping);

        assert_eq!(content.category, ContentCategory::Data);
        assert!(!content.requires_encryption);
        assert_eq!(content.compression_level, 4);
    }

    #[test]
    fn test_executable_categorization() {
        let mapping = FileDataIdMapping::new(67890, "Wow.exe".to_string());
        let content = ContentInfo::new(mapping);

        assert_eq!(content.category, ContentCategory::Executable);
        assert!(content.requires_encryption);
        assert_eq!(content.compression_level, 9);
    }

    #[tokio::test]
    async fn test_orchestrator_creation() {
        let temp_dir = TempDir::new().expect("Test assertion");
        let config = OrchestratorConfig {
            data_dir: temp_dir.path().to_path_buf(),
            enable_metrics: true,
            max_cache_memory: 1024 * 1024,
        };

        let provider = MemoryProvider::empty();
        let orchestrator = MetadataOrchestrator::new(Box::new(provider), config);

        assert!(orchestrator.is_ok());
    }

    #[tokio::test]
    async fn test_orchestrator_operations() {
        let temp_dir = TempDir::new().expect("Test assertion");
        let config = OrchestratorConfig {
            data_dir: temp_dir.path().to_path_buf(),
            enable_metrics: true,
            max_cache_memory: 1024 * 1024,
        };

        let mut provider = MemoryProvider::empty();
        provider.add_mapping(FileDataIdMapping::new(
            12345,
            "Interface/Test.lua".to_string(),
        ));

        let mut orchestrator =
            MetadataOrchestrator::new(Box::new(provider), config).expect("Test assertion");
        orchestrator.load_mappings().await.expect("Test assertion");

        // Test resolutions
        assert_eq!(
            orchestrator
                .resolve_file_path(12345)
                .expect("Test assertion"),
            Some("Interface/Test.lua".to_string())
        );
        assert_eq!(
            orchestrator
                .resolve_file_data_id("Interface/Test.lua")
                .expect("Test assertion"),
            Some(12345)
        );

        // Test content info
        let content_info = orchestrator
            .get_content_info(12345)
            .expect("Test assertion")
            .expect("Test assertion");
        assert_eq!(content_info.mapping.id, 12345);
        assert_eq!(content_info.category, ContentCategory::Data);

        // Test statistics
        let stats = orchestrator.get_stats().expect("should get stats");
        assert_eq!(stats.fdid_stats.total_mappings, 1);
    }

    #[tokio::test]
    async fn test_advanced_orchestrator_features() {
        let temp_dir = TempDir::new().expect("Test assertion");
        let config = OrchestratorConfig {
            data_dir: temp_dir.path().to_path_buf(),
            enable_metrics: true,
            max_cache_memory: 1024 * 1024,
        };

        let mut provider = MemoryProvider::empty();
        provider.add_mapping(FileDataIdMapping::new(1, "Wow.exe".to_string()));
        provider.add_mapping(FileDataIdMapping::new(2, "Interface/Test.lua".to_string()));
        provider.add_mapping(FileDataIdMapping::new(
            3,
            "Sound/Music/test.ogg".to_string(),
        ));

        let mut orchestrator =
            MetadataOrchestrator::new(Box::new(provider), config).expect("Test assertion");
        orchestrator.load_mappings().await.expect("Test assertion");

        // Test bulk resolution
        let bulk_results = orchestrator
            .bulk_resolve_content(&[1, 2, 3, 999])
            .expect("Test assertion");
        assert_eq!(bulk_results.len(), 4);
        assert!(bulk_results[0].1.is_some()); // Wow.exe
        assert!(bulk_results[1].1.is_some()); // Test.lua
        assert!(bulk_results[2].1.is_some()); // test.ogg
        assert!(bulk_results[3].1.is_none()); // 999 (not found)

        // Test validation
        let validation = orchestrator.validate_content(1).expect("Test assertion");
        assert_eq!(validation.file_data_id, 1);
        // Should have an error about missing encryption key for Wow.exe
        assert!(!validation.is_valid);
        assert!(!validation.issues.is_empty());

        // Test validation cache
        let cache_stats = orchestrator.get_validation_cache_stats();
        assert_eq!(cache_stats.cached_validations, 1);

        // Test health status
        let health = orchestrator.get_health_status();
        assert!(health.is_healthy); // No critical issues
        assert!(!health.warnings.is_empty()); // Should have TACT key warning
    }

    #[test]
    fn test_orchestrator_builder() {
        let temp_dir = TempDir::new().expect("Test assertion");
        let provider = MemoryProvider::empty();

        let orchestrator = MetadataOrchestrator::builder()
            .data_dir(temp_dir.path().to_path_buf())
            .enable_metrics(false)
            .max_cache_memory(512 * 1024)
            .fdid_provider(Box::new(provider))
            .build();

        assert!(orchestrator.is_ok());
        let orchestrator = orchestrator.expect("Test assertion");
        assert_eq!(orchestrator.get_config().max_cache_memory, 512 * 1024);
        assert!(!orchestrator.get_config().enable_metrics);
    }

    #[tokio::test]
    async fn test_content_with_encryption() {
        let temp_dir = TempDir::new().expect("Test assertion");
        let config = OrchestratorConfig {
            data_dir: temp_dir.path().to_path_buf(),
            enable_metrics: true,
            max_cache_memory: 1024 * 1024,
        };

        let mut provider = MemoryProvider::empty();
        provider.add_mapping(FileDataIdMapping::new(1, "Wow.exe".to_string()));

        let mut orchestrator =
            MetadataOrchestrator::new(Box::new(provider), config).expect("Test assertion");
        orchestrator.load_mappings().await.expect("Test assertion");

        // Add a test TACT key
        orchestrator
            .add_tact_key(
                0x1234_5678_90AB_CDEF,
                "0123456789ABCDEF0123456789ABCDEF",
                "test",
                Some("Test key".to_string()),
                None,
                None,
            )
            .expect("Test assertion");

        // Test encryption resolution
        let full_content = orchestrator
            .resolve_content_with_encryption(1)
            .expect("Test assertion")
            .expect("Test assertion");
        assert_eq!(full_content.content.mapping.id, 1);
        assert!(full_content.content.requires_encryption);
        assert!(full_content.encryption_available);
        assert!(full_content.encryption_key_id.is_some());
    }
}
