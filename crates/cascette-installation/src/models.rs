//! Data models for installation plans and requests

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

/// Complete installation plan with all metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallationPlan {
    /// Unique plan identifier
    pub id: Uuid,
    /// Plan creation timestamp
    pub created_at: DateTime<Utc>,
    /// Product information
    pub product: ProductInfo,
    /// Build selection
    pub build: BuildSelection,
    /// Configuration files
    pub configs: ConfigurationSet,
    /// System manifests
    pub manifests: ManifestSet,
    /// Archive indices
    pub archives: ArchiveSet,
    /// Installation target
    pub target: InstallationTarget,
}

impl InstallationPlan {
    /// Validate the plan is complete and ready
    #[allow(dead_code)]
    pub fn validate(&self) -> Result<(), String> {
        if self.product.code.is_empty() {
            return Err("Product code cannot be empty".to_string());
        }

        // Check if we have the necessary configuration
        if self.configs.build_config.is_empty() {
            return Err("Build config hash is missing".to_string());
        }

        if self.configs.cdn_config.is_empty() {
            return Err("CDN config hash is missing".to_string());
        }

        Ok(())
    }

    /// Save plan to JSON file
    ///
    /// # Errors
    ///
    /// Returns error if JSON serialization fails or file cannot be written
    pub fn save(&self, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load plan from JSON file
    ///
    /// # Errors
    ///
    /// Returns error if file cannot be read or JSON deserialization fails
    pub fn load(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let json = std::fs::read_to_string(path)?;
        let plan = serde_json::from_str(&json)?;
        Ok(plan)
    }
}

/// User's installation request with configuration parameters
///
/// Specifies what to install, where to install it, and how to handle
/// the installation process (plan-only, execute, or full).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallationRequest {
    /// Product code to install (e.g., "wow", "`wow_classic`")
    pub product_code: String,
    /// Specific build ID for historic builds (None for latest)
    pub build_id: Option<u32>,
    /// Target installation directory
    pub output_dir: PathBuf,
    /// Whether to only create a plan without executing
    pub plan_only: bool,
    /// Path to existing plan file to execute
    pub execute_plan: Option<PathBuf>,
    /// Retry configuration for failed operations
    pub retry_config: RetryConfig,
    /// Cache configuration for downloads
    pub cache_config: CacheConfig,
    /// Maximum concurrent downloads
    pub max_concurrent: usize,
}

impl InstallationRequest {
    /// Validate the request
    #[allow(dead_code)]
    pub fn validate(&self) -> Result<(), String> {
        if self.product_code.is_empty() {
            return Err("Product code cannot be empty".to_string());
        }

        if self.plan_only && self.execute_plan.is_some() {
            return Err("Cannot specify both plan_only and execute_plan".to_string());
        }

        if self.max_concurrent == 0 || self.max_concurrent > 10 {
            return Err("max_concurrent must be between 1 and 10".to_string());
        }

        Ok(())
    }

    /// Get the installation mode
    #[allow(dead_code)] // Future execution mode
    #[must_use]
    pub fn mode(&self) -> InstallationMode {
        if self.plan_only {
            InstallationMode::PlanOnly
        } else if self.execute_plan.is_some() {
            InstallationMode::ExecutePlan
        } else {
            InstallationMode::Full
        }
    }

    /// Check if plan should be created
    #[allow(dead_code)] // Future execution mode
    #[must_use]
    pub fn should_create_plan(&self) -> bool {
        self.plan_only || self.execute_plan.is_none()
    }

    /// Check if plan should be executed
    #[allow(dead_code)] // Future execution mode
    #[must_use]
    pub fn should_execute_plan(&self) -> bool {
        !self.plan_only
    }
}

/// Installation mode determining execution behavior
///
/// Controls whether the installation process creates a plan, executes
/// an existing plan, or performs both steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallationMode {
    /// Only create installation plan without executing
    #[allow(dead_code)] // Future execution mode
    PlanOnly,
    /// Execute an existing installation plan
    #[allow(dead_code)] // Future execution mode
    ExecutePlan,
    /// Create and immediately execute installation plan
    #[allow(dead_code)] // Future execution mode
    Full,
}

/// Product information from NGDP metadata
///
/// Identifies the product being installed including region and channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductInfo {
    /// Product code (e.g., "wow", "`wow_classic`")
    pub code: String,
    /// Human-readable product name
    pub name: String,
    /// Region code (e.g., "us", "eu")
    pub region: String,
    /// Release channel (e.g., "ptr", "beta")
    pub channel: Option<String>,
}

/// Build selection specifying which build to install
///
/// Can reference either the latest build from NGDP or a historic build
/// from community archives.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum BuildSelection {
    /// Latest build from live NGDP query
    Latest {
        /// Version string (e.g., "1.15.7")
        version: String,
        /// Build number
        build_id: u32,
        /// When this build was discovered
        discovered_at: DateTime<Utc>,
    },
    /// Historic build from community archives
    Historic {
        /// Version string (e.g., "1.13.2")
        version: String,
        /// Build number
        build_id: u32,
        /// Source of the build metadata (e.g., "wago.tools")
        source: String,
        /// When this build was imported
        imported_at: DateTime<Utc>,
    },
}

impl BuildSelection {
    /// Check if this is a latest build
    #[must_use]
    pub fn is_latest(&self) -> bool {
        matches!(self, Self::Latest { .. })
    }

    /// Check if this is a historic build
    #[allow(dead_code)] // Used by tests
    #[must_use]
    pub fn is_historic(&self) -> bool {
        matches!(self, Self::Historic { .. })
    }

    /// Get the build ID
    #[must_use]
    pub fn build_id(&self) -> u32 {
        match self {
            Self::Latest { build_id, .. } => *build_id,
            Self::Historic { build_id, .. } => *build_id,
        }
    }

    /// Get the version string
    #[must_use]
    pub fn version(&self) -> &str {
        match self {
            Self::Latest { version, .. } => version,
            Self::Historic { version, .. } => version,
        }
    }

    /// Get the source for historic builds
    #[must_use]
    pub fn source(&self) -> Option<&str> {
        match self {
            Self::Historic { source, .. } => Some(source),
            Self::Latest { .. } => None,
        }
    }

    // NOTE: parse_version method removed - not used
}

/// Retry configuration for network operations
///
/// Controls retry behavior with exponential backoff and jitter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Initial delay before first retry
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Backoff multiplier for each retry
    pub backoff_factor: f32,
    /// Whether to add random jitter to delays
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(1000),
            max_delay: Duration::from_secs(30),
            backoff_factor: 2.0,
            jitter: true,
        }
    }
}

impl RetryConfig {
    /// Validate retry configuration parameters
    ///
    /// # Errors
    ///
    /// Returns an error if configuration values are out of acceptable ranges.
    #[allow(dead_code)]
    pub fn validate(&self) -> Result<(), String> {
        if self.max_attempts == 0 || self.max_attempts > 10 {
            return Err("max_attempts must be between 1 and 10".to_string());
        }

        if self.initial_delay > self.max_delay {
            return Err("initial_delay cannot be greater than max_delay".to_string());
        }

        if self.backoff_factor < 1.0 || self.backoff_factor > 5.0 {
            return Err("backoff_factor must be between 1.0 and 5.0".to_string());
        }

        Ok(())
    }
}

/// Cache configuration for downloaded content
///
/// Controls local caching of CDN downloads to improve performance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Whether caching is enabled
    pub enabled: bool,
    /// Cache directory path
    pub directory: PathBuf,
    /// Maximum cache size in bytes
    pub max_size: u64,
    /// How long to retain cached items
    pub retention: Duration,
    /// Cache eviction policy when full
    pub eviction_policy: EvictionPolicy,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            directory: PathBuf::from("/tmp/cascette-cache"),
            max_size: 10 * 1024 * 1024 * 1024, // 10GB
            retention: Duration::from_secs(7 * 24 * 60 * 60), // 7 days
            eviction_policy: EvictionPolicy::Lru,
        }
    }
}

impl CacheConfig {
    /// Validate cache configuration parameters
    ///
    /// # Errors
    ///
    /// Returns an error if cache settings are invalid when enabled.
    #[allow(dead_code)]
    pub fn validate(&self) -> Result<(), String> {
        if self.enabled && self.max_size == 0 {
            return Err("Cache max_size must be > 0 when enabled".to_string());
        }

        if self.enabled && self.retention.as_secs() == 0 {
            return Err("Cache retention must be > 0 when enabled".to_string());
        }

        Ok(())
    }
}

/// Cache eviction policy for managing full cache
///
/// Determines which items are removed when cache reaches capacity.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum EvictionPolicy {
    /// Least Recently Used
    Lru,
    /// First In First Out
    Fifo,
    /// Time To Live
    Ttl,
}

/// Configuration set - mirrors .build.info structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConfigurationSet {
    /// `BuildConfig` hash (Build Key in .build.info)
    pub build_config: String,
    /// `CDNConfig` hash (CDN Key in .build.info)
    pub cdn_config: String,
    /// Product config hash (if available)
    pub product_config: Option<String>,
    /// Install manifest key (Install Key in .build.info)
    pub install_key: Option<String>,
    /// CDN path for this build
    pub cdn_path: String,
    /// CDN hosts from configuration (can be overridden by cascette config)
    pub cdn_hosts: Vec<String>,
    /// Tags for installation filtering
    pub tags: Vec<String>,
}

/// Build configuration data from CDN
///
/// Contains the raw build configuration file and its hash.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuildConfigData {
    /// Configuration file hash
    pub hash: String,
    /// Raw configuration file data
    pub raw: Vec<u8>,
}

impl BuildConfigData {
    /// Validate build configuration hash format
    ///
    /// # Errors
    ///
    /// Returns an error if hash is not 32 hex characters.
    #[allow(dead_code)]
    pub fn validate(&self) -> Result<(), String> {
        if self.hash.len() != 32 {
            return Err("Hash must be 32 characters".to_string());
        }

        if !self.hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("Hash must contain only hex digits".to_string());
        }

        Ok(())
    }
}

/// CDN configuration data from CDN
///
/// Contains the raw CDN configuration file, its hash, and optional parsed data.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CdnConfigData {
    /// Configuration file hash
    pub hash: String,
    /// Raw configuration file data
    pub raw: Vec<u8>,
    /// Parsed configuration data if available
    pub parsed: Option<CdnConfigParsed>,
}

impl CdnConfigData {
    /// Get the number of archives in the configuration
    #[must_use]
    pub fn archive_count(&self) -> usize {
        self.parsed.as_ref().map_or(0, |p| p.archives.len())
    }

    /// Check if configuration has any archives
    #[allow(dead_code)]
    #[must_use]
    pub fn has_archives(&self) -> bool {
        self.archive_count() > 0
    }
}

/// Parsed CDN configuration with archive lists
///
/// Contains lists of archive hashes and optional archive-group reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdnConfigParsed {
    /// List of archive content keys
    pub archives: Vec<String>,
    /// Archive-group mega-index hash if present
    pub archive_group: Option<String>,
    /// List of patch archive content keys
    pub patch_archives: Vec<String>,
}

/// Patch configuration data from CDN
///
/// Contains the raw patch configuration file and its hash.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PatchConfigData {
    /// Configuration file hash
    pub hash: String,
    /// Raw configuration file data
    pub raw: Vec<u8>,
}

/// Product configuration data from CDN
///
/// Contains the raw product configuration file and its hash.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProductConfigData {
    /// Configuration file hash
    pub hash: String,
    /// Raw configuration file data
    pub raw: Vec<u8>,
}

/// System file references from build configuration
///
/// Contains content keys for required NGDP system files.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemFileRefs {
    /// Root manifest file reference
    pub root: FileRef,
    /// Encoding manifest file reference
    pub encoding: FileRef,
    /// Install manifest file reference
    pub install: FileRef,
    /// Download manifest file reference (optional)
    pub download: Option<FileRef>,
    /// Size manifest file reference (optional)
    pub size: Option<FileRef>,
    /// Patch manifest file reference (optional)
    pub patch: Option<FileRef>,
}

impl SystemFileRefs {
    /// Check if all required system files are present
    #[allow(dead_code)]
    #[must_use]
    pub fn has_required_files(&self) -> bool {
        self.root.content_key != [0u8; 16]
            && self.encoding.content_key != [0u8; 16]
            && self.install.content_key != [0u8; 16]
    }

    /// Calculate total size of all manifests
    #[allow(dead_code)]
    #[must_use]
    pub fn total_manifest_size(&self) -> Option<u64> {
        let mut total = 0u64;

        total += self.root.size.unwrap_or(0);
        total += self.encoding.size.unwrap_or(0);
        total += self.install.size.unwrap_or(0);

        if total > 0 { Some(total) } else { None }
    }
}

/// File reference with content and encoding keys
///
/// Links a file's content key to its encoding key and size.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileRef {
    /// Content key (MD5 hash of file content)
    pub content_key: [u8; 16],
    /// Encoding key (MD5 hash of encoded/compressed data)
    pub encoding_key: Option<[u8; 16]>,
    /// File size in bytes
    pub size: Option<u64>,
}

/// Manifest set containing all NGDP manifests
///
/// Holds metadata for all required and optional manifests in a build.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManifestSet {
    /// Encoding manifest metadata
    pub encoding: EncodingManifest,
    /// Root manifest metadata
    pub root: RootManifest,
    /// Install manifest metadata
    pub install: InstallManifest,
    /// Download manifest metadata (optional)
    pub download: Option<DownloadManifest>,
}

/// Encoding manifest metadata
///
/// Maps content keys to encoding keys for all files in a build.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EncodingManifest {
    /// Encoding manifest encoding key
    pub encoding_key: [u8; 16],
    /// Manifest file size in bytes
    pub size: u64,
    // NOTE: parsed field removed - was never read
    /// Number of entries in manifest
    pub entry_count: usize,
}

/// Root manifest metadata
///
/// Maps file paths to content keys for installed files.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RootManifest {
    /// Root manifest content key
    pub content_key: [u8; 16],
    /// Root manifest encoding key
    pub encoding_key: [u8; 16],
    /// Manifest file size in bytes
    pub size: u64,
    /// Root manifest format version
    pub version: RootVersion,
    /// Number of files in manifest
    pub file_count: usize,
    // NOTE: parsed field removed - was never read
}

/// Root file version indicating format version
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub enum RootVersion {
    /// Version 1 root format
    V1,
    /// Version 2 root format
    V2,
    /// Version 3 root format
    V3,
    /// Version 4 root format (current)
    #[default]
    V4,
}

impl From<cascette_formats::root::RootVersion> for RootVersion {
    fn from(version: cascette_formats::root::RootVersion) -> Self {
        match version {
            cascette_formats::root::RootVersion::V1 => Self::V1,
            cascette_formats::root::RootVersion::V2 => Self::V2,
            cascette_formats::root::RootVersion::V3 => Self::V3,
            cascette_formats::root::RootVersion::V4 => Self::V4,
        }
    }
}

/// Install manifest metadata
///
/// Specifies which files to install and their sizes.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstallManifest {
    /// Install manifest content key
    pub content_key: [u8; 16],
    /// Install manifest encoding key
    pub encoding_key: [u8; 16],
    /// Manifest file size in bytes
    pub size: u64,
    /// Number of files to install
    pub file_count: usize,
    /// Total size of installed files in bytes
    pub total_install_size: u64,
    /// Installation tags for filtering
    pub tags: Vec<String>,
    // NOTE: parsed field removed - was never read
}

/// Download manifest metadata
///
/// Specifies differential download requirements for updates.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DownloadManifest {
    /// Download manifest content key
    pub content_key: [u8; 16],
    /// Download manifest encoding key
    pub encoding_key: [u8; 16],
    /// Manifest file size in bytes
    pub size: u64,
}

/// Archive set containing all archive metadata
///
/// Contains information about CDN archives and their indices.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArchiveSet {
    /// List of archives
    pub archives: Vec<ArchiveInfo>,
    /// List of archive indices
    pub indices: Vec<ArchiveIndexInfo>,
    /// Total size of all archives in bytes
    pub total_archive_size: u64,
}

/// Archive information from CDN
///
/// Metadata for a single CDN archive file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveInfo {
    /// Archive content hash
    pub hash: String,
    /// Archive file size in bytes
    pub size: u64,
}

/// Archive index information
///
/// Metadata for a single archive index file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveIndexInfo {
    /// Associated archive hash
    pub archive_hash: String,
    /// Index file size in bytes
    pub index_size: u64,
    /// Number of entries in index
    pub entry_count: usize,
}

/// Installation target configuration
///
/// Specifies where and how to install the product.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallationTarget {
    /// Target installation directory
    pub directory: PathBuf,
    /// Target platform
    pub platform: Platform,
    /// Installation filter tags
    pub tags: Vec<String>,
}

/// Target platform for installation
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Platform {
    /// Windows platform
    Windows,
    /// macOS platform
    MacOS,
    /// Linux platform (experimental)
    Linux,
}

/// Installation summary with size and readiness information
///
/// Contains aggregate statistics about an installation plan.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstallationSummary {
    /// Total download size in bytes
    pub total_download_size: u64,
    /// Total install size in bytes
    pub total_install_size: u64,
    /// Number of files to install
    pub file_count: usize,
    /// CDN hosts for downloads
    pub cdn_hosts: Vec<String>,
    /// CDN path prefix
    pub cdn_path: String,
    /// Whether plan is ready to execute
    pub ready_to_execute: bool,
}

/// Resume data for interrupted downloads
///
/// Tracks progress of an installation to support resumption after interruption.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResumeData {
    /// Unique installation identifier
    pub installation_id: Uuid,
    /// Set of completed file paths
    pub completed_files: HashSet<PathBuf>,
    /// Partially downloaded files with byte offsets
    pub partial_files: HashMap<PathBuf, u64>,
    /// Remaining download tasks
    pub pending_tasks: Vec<DownloadTask>,
    /// Installation metadata
    pub metadata: InstallationManifest,
}

impl ResumeData {
    /// Save resume data to disk
    ///
    /// # Errors
    ///
    /// Returns an error if file creation or serialization fails.
    #[allow(dead_code)] // Future resume support
    pub fn save(&self, cache_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let resume_dir = cache_dir.join("resume");
        std::fs::create_dir_all(&resume_dir)?;

        let resume_path = resume_dir.join(format!("{}.json", self.installation_id));
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(resume_path, json)?;

        Ok(())
    }

    /// Load resume data from disk
    ///
    /// # Errors
    ///
    /// Returns an error if file read or deserialization fails.
    #[allow(dead_code)] // Future resume support
    pub fn load(cache_dir: &Path, id: Uuid) -> Result<Self, Box<dyn std::error::Error>> {
        let resume_path = cache_dir.join(format!("resume/{id}.json"));
        let json = std::fs::read_to_string(resume_path)?;
        let data = serde_json::from_str(&json)?;
        Ok(data)
    }
}

/// Download task for concurrent execution
///
/// Represents a single file download operation with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadTask {
    /// Unique task identifier
    pub id: Uuid,
    /// CDN URL to download from
    pub url: String,
    /// Target file path
    pub target_path: PathBuf,
    /// Expected file size in bytes
    pub expected_size: Option<u64>,
    /// Expected file checksum for verification
    pub expected_checksum: Option<String>,
    /// Task priority for scheduling
    pub priority: u32,
}

/// Installation manifest for post-installation record
///
/// Records information about a completed installation for tracking and verification.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstallationManifest {
    /// Product code that was installed
    pub product_code: String,
    /// Build ID that was installed
    pub build_id: String,
    /// Installation directory
    pub install_dir: PathBuf,
    /// Installation timestamp
    pub installed_at: DateTime<Utc>,
    /// Total installed size in bytes
    pub total_size: u64,
    /// Number of installed files
    pub file_count: usize,
}
