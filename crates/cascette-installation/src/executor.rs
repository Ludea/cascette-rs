//! Plan execution module for actual file downloads and installation
//!
//! This module implements F006-F008:
//! - F006: Actual file download from archives
//! - F007: BLTE decryption and decompression
//! - F008: File extraction to target directory

use crate::{
    archive_optimizer::ArchiveBlockMap,
    battlenet::BattlenetInstaller,
    error::{InstallationError, Result},
    models::InstallationPlan,
    progress::ProgressCallback,
    progress_tracker::PersistentProgressCallback,
    resume::ResumeManager,
    retry::RetryExecutor,
};
use cascette_client_storage::archive::ArchiveManager;
use cascette_crypto::{ContentKey, EncodingKey};
use cascette_formats::{
    CascFormat,
    archive::{ArchiveIndex, IndexEntry},
    blte::BlteFile,
    config::CdnConfig as FormatsCdnConfig,
    encoding::EncodingFile,
    install::InstallManifest as FormatsInstallManifest,
};
use cascette_protocol::{
    CdnClient, CdnConfig as ProtocolCdnConfig, CdnEndpoint, ClientConfig, ContentType,
    RibbitTactClient,
};
use futures::stream::{self, StreamExt};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use tokio::time;

/// Installation mode controls the output structure and behavior
///
/// Battle.net mode creates a complete CASC storage structure compatible
/// with official game clients. Simple mode extracts files to a flat
/// directory for testing and debugging purposes only.
///
/// # Examples
///
/// ```
/// use cascette_installation::executor::InstallationMode;
///
/// let mode = InstallationMode::default();
/// assert_eq!(mode, InstallationMode::Battlenet);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallationMode {
    /// Battle.net-compatible installation (default)
    ///
    /// Creates full CASC structure with:
    /// - Data/ directory (config/, data/, indices/)
    /// - Product-specific directory (_retail_, _classic_, etc.)
    /// - Metadata files (.build.info, .product.db, Launcher.db)
    /// - Local index files (.idx)
    /// - Archive-group mega-indices
    ///
    /// This mode works for both game and tool products.
    Battlenet,

    /// Simple extraction mode (testing/debugging only)
    ///
    /// Extracts files to flat directory structure without CASC metadata.
    ///
    /// **WARNING**: Games will NOT run in this mode. Use only for:
    /// - File extraction and inspection
    /// - Testing file download logic
    /// - Debugging content resolution
    Simple,
}

impl Default for InstallationMode {
    fn default() -> Self {
        Self::Battlenet
    }
}

/// Executes installation plans by downloading and extracting files
///
/// The plan executor orchestrates the complete installation process including:
/// - Downloading CDN configurations and manifests
/// - Downloading archive indices
/// - Extracting files from archives
/// - Creating Battle.net-compatible directory structures
/// - Generating metadata files and local indices
///
/// Supports both Battle.net mode (creates full CASC storage) and Simple mode
/// (extracts files to flat directory for testing).
pub struct PlanExecutor {
    cdn_client: Arc<CdnClient>,
    progress_callback: Option<Box<dyn ProgressCallback>>,
    installation_mode: InstallationMode,
    resume_manager: Option<ResumeManager>,
    archive_manager: Option<Arc<Mutex<ArchiveManager>>>,
    retry_executor: RetryExecutor,
}

impl PlanExecutor {
    /// Create a new plan executor with default CDN client and archive manager
    ///
    /// Initializes the CDN client with a cache directory and sets up the archive manager
    /// for local storage. The executor is created with default Battle.net installation mode.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache directory cannot be created or the CDN client
    /// initialization fails.
    pub fn new() -> Result<Self> {
        // Initialize CDN client with cache
        let cache_dir = dirs::cache_dir()
            .ok_or_else(|| {
                InstallationError::NetworkError("Could not determine cache directory".to_string())
            })?
            .join("cascette");

        std::fs::create_dir_all(&cache_dir).map_err(|e| {
            InstallationError::NetworkError(format!("Failed to create cache directory: {e}"))
        })?;

        let mut config = ClientConfig::default();
        config.cache_config.cache_dir = Some(cache_dir.clone());

        let protocol_client = Arc::new(
            RibbitTactClient::new(config)
                .map_err(|e| InstallationError::NetworkError(e.to_string()))?,
        );

        // Create CDN client using shared cache from protocol client
        let client = Arc::new(
            CdnClient::new(
                protocol_client.cache().clone(),
                ProtocolCdnConfig::default(),
            )
            .map_err(|e| InstallationError::NetworkError(e.to_string()))?,
        );

        // Initialize the archive manager with a local archives cache directory
        let archives_dir = cache_dir.join("local_archives");
        std::fs::create_dir_all(&archives_dir).map_err(|e| {
            InstallationError::NetworkError(format!("Failed to create archives directory: {e}"))
        })?;

        let archive_manager = Arc::new(Mutex::new(ArchiveManager::new(&archives_dir)));

        // Create retry executor with default configuration
        let retry_executor = RetryExecutor::new();

        Ok(Self {
            cdn_client: client,
            progress_callback: None,
            installation_mode: InstallationMode::default(),
            resume_manager: None,
            archive_manager: Some(archive_manager),
            retry_executor,
        })
    }

    /// Get the current installation mode
    #[allow(dead_code)] // Used in tests
    #[must_use]
    pub fn installation_mode(&self) -> InstallationMode {
        self.installation_mode
    }

    /// Download data with retry logic and exponential backoff
    ///
    /// # Arguments
    ///
    /// * `endpoint` - CDN endpoint to download from
    /// * `content_type` - Type of content being downloaded (Config, Data, etc.)
    /// * `key` - Content key to download
    /// * `operation_name` - Human-readable operation name for logging
    ///
    /// # Errors
    ///
    /// Returns an error if all retry attempts fail.
    async fn download_with_retry(
        &self,
        endpoint: &CdnEndpoint,
        content_type: ContentType,
        key: &[u8],
        operation_name: &str,
    ) -> Result<Vec<u8>> {
        let cdn_client = self.cdn_client.clone();
        let endpoint_clone = endpoint.clone();
        let key_vec = key.to_vec();

        self.retry_executor
            .execute_async(
                move || {
                    let client = cdn_client.clone();
                    let ep = endpoint_clone.clone();
                    let k = key_vec.clone();
                    async move {
                        client
                            .download(&ep, content_type, &k)
                            .await
                            .map_err(|e| InstallationError::NetworkError(e.to_string()))
                    }
                },
                operation_name,
            )
            .await
    }

    /// Download range with retry logic and exponential backoff
    ///
    /// Downloads a specific byte range from a CDN resource with automatic retries.
    ///
    /// # Arguments
    ///
    /// * `endpoint` - CDN endpoint to download from
    /// * `content_type` - Type of content being downloaded
    /// * `key` - Content key to download
    /// * `offset` - Byte offset to start downloading from
    /// * `size` - Number of bytes to download
    /// * `operation_name` - Human-readable operation name for logging
    ///
    /// # Errors
    ///
    /// Returns an error if all retry attempts fail.
    #[allow(dead_code)] // Will be used for archive range downloads
    async fn download_range_with_retry(
        &self,
        endpoint: &CdnEndpoint,
        content_type: ContentType,
        key: &[u8],
        offset: u64,
        size: u64,
        operation_name: &str,
    ) -> Result<Vec<u8>> {
        let cdn_client = self.cdn_client.clone();
        let endpoint_clone = endpoint.clone();
        let key_vec = key.to_vec();

        self.retry_executor
            .execute_async(
                move || {
                    let client = cdn_client.clone();
                    let ep = endpoint_clone.clone();
                    let k = key_vec.clone();
                    async move {
                        client
                            .download_range(&ep, content_type, &k, offset, size)
                            .await
                            .map_err(|e| InstallationError::NetworkError(e.to_string()))
                    }
                },
                operation_name,
            )
            .await
    }

    /// Set progress callback for monitoring download progress
    ///
    /// Provides a custom callback for tracking installation progress. The callback
    /// will receive notifications for file downloads, completion, and errors.
    ///
    /// # Arguments
    ///
    /// * `callback` - Progress callback implementation
    ///
    /// # Returns
    ///
    /// Self for builder pattern chaining
    #[must_use]
    #[allow(dead_code)]
    pub fn with_progress_callback(mut self, callback: Box<dyn ProgressCallback>) -> Self {
        self.progress_callback = Some(callback);
        self
    }

    /// Use persistent progress tracking (recommended for large installations)
    ///
    /// Enables progress tracking that persists across interruptions, allowing installations
    /// to resume from the last successful state. This is the recommended approach for
    /// large installations.
    ///
    /// # Arguments
    ///
    /// * `plan` - Installation plan for tracking context
    /// * `verbose` - Whether to output verbose progress information
    ///
    /// # Returns
    ///
    /// Self for builder pattern chaining
    #[must_use]
    pub fn with_persistent_progress(mut self, plan: &InstallationPlan, verbose: bool) -> Self {
        let callback = PersistentProgressCallback::new(
            &plan.target.directory,
            plan.manifests.install.file_count,
            plan.manifests.install.total_install_size,
            verbose,
        );
        self.progress_callback = Some(Box::new(callback));
        self
    }

    /// Override the installation mode
    ///
    /// WARNING: Simple mode does not create working game installations.
    /// Only use this for file extraction or debugging purposes.
    ///
    /// # Arguments
    /// * `mode` - The installation mode to use
    ///
    /// # Returns
    /// Self for builder pattern chaining
    ///
    /// # Examples
    ///
    /// ```
    /// use cascette_installation::executor::{InstallationMode, PlanExecutor};
    ///
    /// let executor = PlanExecutor::new()
    ///     .unwrap()
    ///     .with_installation_mode(InstallationMode::Simple);
    /// ```
    #[must_use]
    pub fn with_installation_mode(mut self, mode: InstallationMode) -> Self {
        self.installation_mode = mode;
        self
    }

    /// Execute an installation plan
    ///
    /// Downloads and installs all files according to the plan. The installation mode
    /// determines the output structure (Battle.net-compatible or simple extraction).
    ///
    /// # Arguments
    ///
    /// * `plan` - Installation plan to execute
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Plan validation fails
    /// - CDN downloads fail
    /// - File extraction fails
    /// - Directory creation fails
    ///
    /// # Panics
    ///
    /// Does not panic under normal operation.
    pub async fn execute_plan(&mut self, plan: &InstallationPlan) -> Result<()> {
        // Validate the plan first
        plan.validate().map_err(InstallationError::Other)?;

        // Clean up any orphaned temporary files from previous interrupted installations
        if let Ok(cleaned) =
            crate::atomic_writer::cleanup_temp_files_recursive(&plan.target.directory)
        {
            if cleaned > 0 {
                println!("→ Cleaned up {cleaned} temporary files from previous installation");
            }
        }

        println!("→ Executing installation plan for {}", plan.product.name);
        println!(
            "  Build: {} ({})",
            plan.build.version(),
            plan.build.build_id()
        );
        println!("  Target: {}", plan.target.directory.display());
        println!(
            "  Mode: {}",
            match self.installation_mode {
                InstallationMode::Battlenet => "Battle.net-compatible",
                InstallationMode::Simple => "Simple extraction (WARNING: Game will not run)",
            }
        );
        println!("  Files: {} files", plan.manifests.install.file_count);
        println!(
            "  Install size: {:.1} GB",
            plan.manifests.install.total_install_size as f64 / 1_000_000_000.0
        );

        // If in Battle.net mode, create the proper structure and metadata
        match self.installation_mode {
            InstallationMode::Battlenet => self.execute_battlenet_installation(plan).await,
            InstallationMode::Simple => self.execute_simple_extraction(plan).await,
        }
    }

    /// Execute a simple extraction (TESTING/DEBUGGING ONLY)
    ///
    /// WARNING: This mode extracts files to a flat directory without creating
    /// the proper Battle.net structure. Games will NOT run in this mode.
    /// Only use this for file extraction or debugging purposes.
    ///
    /// This method is renamed from `execute_standard_installation` to clarify
    /// its limited purpose and discourage production use.
    async fn execute_simple_extraction(&mut self, plan: &InstallationPlan) -> Result<()> {
        println!("⚠️  WARNING: Simple extraction mode does not create a working installation");
        println!("   Games will NOT be playable from this directory");
        println!("   This mode is for file extraction and debugging only\n");
        // Create target directory
        fs::create_dir_all(&plan.target.directory).map_err(|e| {
            InstallationError::Other(format!("Failed to create target directory: {e}"))
        })?;

        // Step 1: Load and parse install manifest to get file list
        let install_files = self.get_install_file_list(plan).await?;

        // Step 2: Load encoding manifest for content key → encoding key lookups
        let encoding_file = self.load_encoding_manifest(plan).await?;

        // Step 3: Download CDN archive index files (.index files from CDN)
        let archive_indices = self.download_cdn_archive_indices(plan).await?;

        // Step 4: Download and extract files from archives
        self.download_and_extract_files(plan, &install_files, &encoding_file, &archive_indices)
            .await?;

        println!("✓ Simple extraction completed (not a working installation)");
        Ok(())
    }

    /// Execute a Battle.net-compatible installation
    ///
    /// Creates a full CASC storage structure compatible with official game clients.
    /// This includes all necessary metadata files, local indices, and archive-groups.
    ///
    /// # Errors
    ///
    /// Returns an error if any installation step fails.
    async fn execute_battlenet_installation(&mut self, plan: &InstallationPlan) -> Result<()> {
        let mut installer = BattlenetInstaller::new();

        // Step 1: Initialize Battle.net directory structure
        installer.initialize_structure(plan, &plan.target.directory)?;

        // Step 2: Generate metadata files
        installer.generate_build_info(plan, &plan.target.directory)?;
        installer.generate_product_db(plan, &plan.target.directory)?;
        installer.generate_metadata_files(plan, &plan.target.directory)?;

        // Step 3: Download and store CDN configs
        let cdn_config_data = self.download_cdn_config(plan).await?;
        let build_config_data = self.download_build_config(plan).await?;
        installer.store_cdn_configs(
            plan,
            &plan.target.directory,
            &cdn_config_data,
            &build_config_data,
        )?;

        // Step 4: Download and store archive indices in batches to avoid memory issues
        self.download_and_store_archive_indices_streaming(plan, &plan.target.directory)
            .await?;

        // Step 5: Generate local .idx files
        installer.generate_local_indices(plan, &plan.target.directory)?;

        // Step 5.5: Generate archive-group mega-index
        installer.generate_archive_group(plan, &plan.target.directory)?;

        // Step 6: Download and store game files
        // WoW games use product subdirectories (_retail_/, _classic_/, etc.)
        // Other products (agent, bna) install directly to root
        let (product_path, display_path) = match plan.product.code.as_str() {
            "wow" => {
                let dir = "_retail_";
                let path = plan.target.directory.join(dir);
                (path, format!("{dir}/"))
            }
            "wow_classic" => {
                let dir = "_classic_";
                let path = plan.target.directory.join(dir);
                (path, format!("{dir}/"))
            }
            "wow_classic_era" => {
                let dir = "_classic_era_";
                let path = plan.target.directory.join(dir);
                (path, format!("{dir}/"))
            }
            // For other products (agent, bna, etc.), install directly to root
            _ => (plan.target.directory.clone(), "root directory".to_string()),
        };

        fs::create_dir_all(&product_path).map_err(|e| {
            InstallationError::Other(format!("Failed to create installation directory: {e}"))
        })?;

        // Step 7: Extract game files
        println!("→ Extracting files to {display_path}...");

        // Load install manifest to get file list
        let install_files = self.get_install_file_list(plan).await?;

        // Load encoding manifest for content key lookups
        let encoding_file = self.load_encoding_manifest(plan).await?;

        // Use the stored archive indices from Data/indices/
        let archive_indices = self.load_stored_archive_indices(&plan.target.directory)?;

        // Extract files to product directory
        self.download_and_extract_files_to_directory(
            plan,
            &install_files,
            &encoding_file,
            &archive_indices,
            &product_path,
        )
        .await?;

        println!("✓ Battle.net-compatible installation completed");
        Ok(())
    }

    /// Get the list of files to install from the install manifest
    ///
    /// Downloads and parses the install manifest, then filters files based on target tags.
    ///
    /// # Errors
    ///
    /// Returns an error if manifest download or parsing fails.
    async fn get_install_file_list(
        &self,
        plan: &InstallationPlan,
    ) -> Result<Vec<cascette_formats::install::InstallFileEntry>> {
        // Re-download install manifest from CDN (cache will make this efficient)
        println!("→ Downloading install manifest from CDN...");

        // Use community CDN fallback for historic builds
        let cdn_hosts = self.get_cdn_hosts_with_fallback(plan);
        let cdn_endpoint = CdnEndpoint {
            host: cdn_hosts
                .first()
                .ok_or_else(|| InstallationError::Other("No CDN hosts available".to_string()))?
                .clone(),
            path: plan.configs.cdn_path.clone(),
            product_path: None,
            scheme: None,
        };

        // First load encoding manifest to get install manifest encoding key
        let encoding_file = self.load_encoding_manifest(plan).await?;
        let install_content_key = &plan.manifests.install.content_key;

        // Look up encoding key for install manifest
        let content_key = ContentKey::from_bytes(*install_content_key);
        let encoding_key = encoding_file.find_encoding(&content_key).ok_or_else(|| {
            InstallationError::Other(format!(
                "Could not find encoding key for install manifest content key: {}",
                hex::encode(install_content_key)
            ))
        })?;

        // Download install manifest data using encoding key
        let compressed_data = self
            .download_with_retry(
                &cdn_endpoint,
                ContentType::Data,
                encoding_key.as_bytes(),
                "install manifest",
            )
            .await?;

        // BLTE decompress the install manifest data
        let blte_file = BlteFile::parse(&compressed_data).map_err(|e| {
            InstallationError::InvalidConfiguration(format!(
                "Failed to parse BLTE data for install manifest: {e}"
            ))
        })?;

        let install_data = blte_file.decompress().map_err(|e| {
            InstallationError::InvalidConfiguration(format!(
                "Failed to decompress BLTE data for install manifest: {e}"
            ))
        })?;

        // Parse install manifest
        let install_manifest = FormatsInstallManifest::parse(&install_data).map_err(|e| {
            InstallationError::InvalidConfiguration(format!(
                "Failed to parse install manifest: {e}"
            ))
        })?;

        // Apply the same filtering as during plan creation
        let filtered_files = self.filter_install_entries(&install_manifest, &plan.target.tags);

        println!("→ Found {} files to install", filtered_files.len());
        Ok(filtered_files.into_iter().cloned().collect())
    }

    /// Filter install manifest entries using multi-layer platform detection
    ///
    /// Applies path-based detection (highest priority) and tag-based filtering to determine
    /// which files should be installed for the target platform.
    ///
    /// # Arguments
    ///
    /// * `install_manifest` - Parsed install manifest
    /// * `target_tags` - Target platform and feature tags
    ///
    /// # Returns
    ///
    /// Filtered list of install entries matching the target platform
    #[allow(clippy::case_sensitive_file_extension_comparisons)] // We're using path_lower, so it's already case-insensitive
    fn filter_install_entries<'a>(
        &self,
        install_manifest: &'a FormatsInstallManifest,
        target_tags: &[String],
    ) -> Vec<&'a cascette_formats::install::InstallFileEntry> {
        let entries = &install_manifest.entries;
        let tags = &install_manifest.tags;

        // If no tags specified, return all entries
        if target_tags.is_empty() {
            return entries.iter().collect();
        }

        // Determine the target platform
        let is_targeting_windows = target_tags.contains(&"Windows".to_string());
        let is_targeting_macos = target_tags.contains(&"OSX".to_string());

        // For each entry, apply multi-layer filtering
        let mut matching_entries = Vec::new();

        for (entry_index, entry) in entries.iter().enumerate() {
            // Collect file's tags
            let file_tags: Vec<String> = tags
                .iter()
                .filter(|tag| tag.has_file(entry_index))
                .map(|tag| tag.name.clone())
                .collect();

            // Layer 1: Path-based detection (HIGHEST PRIORITY)
            let path_lower = entry.path.to_lowercase();
            let is_definitely_macos = entry.path.contains(".app\\")
                || entry.path.contains(".app/")
                || entry.path.contains(".framework\\")
                || entry.path.contains(".framework/")
                || path_lower.ends_with(".dylib")
                || path_lower.ends_with(".icns");

            let is_definitely_windows = path_lower.ends_with(".exe")
                || path_lower.ends_with(".dll")
                || path_lower.ends_with(".mfil")
                || path_lower.ends_with(".bat");

            // Layer 2: Tag-based detection
            let has_windows_tag = file_tags.contains(&"Windows".to_string());
            let has_osx_tag = file_tags.contains(&"OSX".to_string());

            // Layer 3: Decision logic with path override
            let mut should_include = false;

            if is_targeting_windows {
                if is_definitely_macos {
                    should_include = false;
                } else if is_definitely_windows {
                    should_include = true;
                } else {
                    // Path is ambiguous, use tag logic with OR semantics
                    should_include = has_windows_tag || !has_osx_tag;
                }
            } else if is_targeting_macos {
                if is_definitely_windows {
                    should_include = false;
                } else if is_definitely_macos {
                    should_include = true;
                } else {
                    // Path is ambiguous, use tag logic with OR semantics
                    should_include = has_osx_tag || !has_windows_tag;
                }
            }

            // Check non-platform tags (locale, architecture) with AND logic
            // Optional feature tags (speech, text) are skipped to allow core files
            // without these tags to be included (T083b fix)
            if should_include {
                // Define optional feature tags that should not exclude files if missing
                let optional_feature_tags = ["speech", "text"];

                for target_tag in target_tags {
                    // Skip platform tags - already handled above
                    if target_tag == "Windows" || target_tag == "OSX" {
                        continue;
                    }

                    // Skip optional feature tags - files without these should still be included
                    // This allows core game files to be installed even without speech/text tags
                    if optional_feature_tags.contains(&target_tag.as_str()) {
                        continue;
                    }

                    // For required tags (locale, architecture), require exact match
                    if let Some(manifest_tag) = tags.iter().find(|t| &t.name == target_tag) {
                        if !manifest_tag.has_file(entry_index) {
                            should_include = false;
                            break;
                        }
                    }
                }
            }

            if should_include {
                matching_entries.push(entry);
            }
        }

        matching_entries
    }

    /// Load and parse the encoding manifest for content key lookups
    ///
    /// Downloads the encoding manifest from CDN and decompresses it for use in
    /// resolving content keys to encoding keys.
    ///
    /// # Errors
    ///
    /// Returns an error if download, decompression, or parsing fails.
    async fn load_encoding_manifest(&self, plan: &InstallationPlan) -> Result<EncodingFile> {
        // Re-download encoding manifest from CDN (cache will make this efficient)
        println!("→ Downloading encoding manifest from CDN...");

        // Use community CDN fallback for historic builds
        let cdn_hosts = self.get_cdn_hosts_with_fallback(plan);
        let cdn_endpoint = CdnEndpoint {
            host: cdn_hosts
                .first()
                .ok_or_else(|| InstallationError::Other("No CDN hosts available".to_string()))?
                .clone(),
            path: plan.configs.cdn_path.clone(),
            product_path: None,
            scheme: None,
        };

        // Download encoding manifest data using encoding key from plan
        let compressed_data = self
            .download_with_retry(
                &cdn_endpoint,
                ContentType::Data,
                &plan.manifests.encoding.encoding_key,
                "encoding manifest",
            )
            .await?;

        // BLTE decompress the encoding manifest data
        let blte_file = BlteFile::parse(&compressed_data).map_err(|e| {
            InstallationError::InvalidConfiguration(format!(
                "Failed to parse BLTE data for encoding manifest: {e}"
            ))
        })?;

        let encoding_data = blte_file.decompress().map_err(|e| {
            InstallationError::InvalidConfiguration(format!(
                "Failed to decompress BLTE data for encoding manifest: {e}"
            ))
        })?;

        // Parse encoding manifest
        EncodingFile::parse(&encoding_data).map_err(|e| {
            InstallationError::InvalidConfiguration(format!(
                "Failed to parse encoding manifest: {e}"
            ))
        })
    }

    /// Download CDN archive index files from the CDN
    ///
    /// Downloads all archive index files (.index) referenced in the CDN config.
    /// These indices map encoding keys to archive offsets for file extraction.
    ///
    /// # Returns
    ///
    /// Vector of tuples containing (`archive_hash`, `parsed_index`)
    ///
    /// # Errors
    ///
    /// Returns an error if CDN config download or index parsing fails.
    async fn download_cdn_archive_indices(
        &self,
        plan: &InstallationPlan,
    ) -> Result<Vec<(String, ArchiveIndex)>> {
        println!("→ Downloading archive index files from CDN...");

        // First download and parse the CDN config to get archive index list
        let cdn_config_data = self.download_cdn_config(plan).await?;
        let cdn_config =
            FormatsCdnConfig::parse(std::io::Cursor::new(&cdn_config_data)).map_err(|e| {
                InstallationError::InvalidConfiguration(format!("Failed to parse CDN config: {e}"))
            })?;

        let mut archive_indices = Vec::new();

        // Use community CDN fallback for historic builds
        let cdn_hosts = self.get_cdn_hosts_with_fallback(plan);
        let cdn_endpoint = CdnEndpoint {
            host: cdn_hosts
                .first()
                .ok_or_else(|| InstallationError::Other("No CDN hosts available".to_string()))?
                .clone(),
            path: plan.configs.cdn_path.clone(),
            product_path: None,
            scheme: None,
        };

        // Download each archive index file
        // Archive indices are separate files with .index suffix
        let archives = cdn_config.archives();
        let total_archives = archives.len();
        println!("  Total archives to download: {total_archives}");

        for (i, archive_info) in archives.into_iter().enumerate() {
            println!(
                "  → Downloading archive index {}/{}: {}.index",
                i + 1,
                total_archives,
                &archive_info.content_key
            );

            // Use the CDN client's dedicated method for downloading index files
            let compressed_data = self
                .cdn_client
                .download_archive_index(&cdn_endpoint, &archive_info.content_key)
                .await
                .map_err(|e| {
                    InstallationError::NetworkError(format!(
                        "Failed to download archive index {}.index: {}",
                        &archive_info.content_key, e
                    ))
                })?;

            // CDN archive indices are typically not BLTE-compressed
            // Try parsing as raw CDN index format first
            let archive_index = match ArchiveIndex::parse(std::io::Cursor::new(&compressed_data)) {
                Ok(cdn_index) => {
                    // Success with raw data (most common case)
                    cdn_index
                }
                Err(raw_err) => {
                    // Raw parsing failed, try BLTE decompression (rare but possible)
                    match BlteFile::parse(&compressed_data) {
                        Ok(blte_file) => {
                            let index_data = blte_file.decompress().map_err(|e| {
                                InstallationError::InvalidConfiguration(format!(
                                    "Failed to decompress BLTE data for archive index {}: {}",
                                    &archive_info.content_key, e
                                ))
                            })?;

                            // Parse the decompressed CDN index
                            ArchiveIndex::parse(std::io::Cursor::new(index_data)).map_err(|e| {
                                InstallationError::InvalidConfiguration(format!(
                                    "Failed to parse CDN index after BLTE decompression: {e}"
                                ))
                            })?
                        }
                        Err(_) => {
                            // Neither raw nor BLTE format worked
                            return Err(InstallationError::InvalidConfiguration(format!(
                                "Archive index {} could not be parsed as CDN index: {}",
                                &archive_info.content_key, raw_err
                            )));
                        }
                    }
                }
            };

            archive_indices.push((archive_info.content_key.clone(), archive_index));
        }

        println!("✓ Downloaded {} archive index files", archive_indices.len());
        Ok(archive_indices)
    }

    /// Load stored archive indices from Data/indices/ directory
    ///
    /// Reads previously downloaded archive index files from the local Data/indices/
    /// directory for use in Battle.net mode installations.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be read.
    fn load_stored_archive_indices(&self, base_dir: &Path) -> Result<Vec<(String, ArchiveIndex)>> {
        let indices_dir = base_dir.join("Data/indices");
        let mut archive_indices = Vec::new();

        // Read all .index files from the directory
        let entries = fs::read_dir(&indices_dir).map_err(|e| {
            InstallationError::Other(format!("Failed to read indices directory: {e}"))
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                InstallationError::Other(format!("Failed to read directory entry: {e}"))
            })?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("index") {
                // Extract hash from filename (without .index extension)
                let hash = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| {
                        InstallationError::Other(format!("Invalid index filename: {path:?}"))
                    })?
                    .to_string();

                // Read and parse the index file
                let data = fs::read(&path).map_err(|e| {
                    InstallationError::Other(format!("Failed to read index file: {e}"))
                })?;

                // Try to parse as archive index
                // Skip files that fail to parse (e.g., archive-group mega-indices)
                match ArchiveIndex::parse(std::io::Cursor::new(&data)) {
                    Ok(archive_index) => {
                        archive_indices.push((hash, archive_index));
                    }
                    Err(e) => {
                        // Skip this file - likely an archive-group or other format
                        eprintln!("  Note: Skipping {hash} (not a regular archive index: {e})");
                    }
                }
            }
        }

        Ok(archive_indices)
    }

    /// Download and extract files to a specific directory
    ///
    /// Helper method that modifies the plan target directory and calls the main
    /// extraction method. Used for extracting to product subdirectories.
    ///
    /// # Errors
    ///
    /// Returns an error if extraction fails.
    async fn download_and_extract_files_to_directory(
        &mut self,
        plan: &InstallationPlan,
        install_files: &[cascette_formats::install::InstallFileEntry],
        encoding_file: &EncodingFile,
        archive_indices: &[(String, ArchiveIndex)],
        target_dir: &Path,
    ) -> Result<()> {
        // Create a modified plan with the product directory as target
        let mut modified_plan = plan.clone();
        modified_plan.target.directory = target_dir.to_path_buf();

        // Call the existing extraction method with modified plan
        self.download_and_extract_files(
            &modified_plan,
            install_files,
            encoding_file,
            archive_indices,
        )
        .await?;

        Ok(())
    }

    /// Download CDN config data from the CDN
    ///
    /// # Errors
    ///
    /// Returns an error if download fails or hash is invalid.
    async fn download_cdn_config(&self, plan: &InstallationPlan) -> Result<Vec<u8>> {
        // Use community CDN fallback for historic builds
        let cdn_hosts = self.get_cdn_hosts_with_fallback(plan);
        let cdn_endpoint = CdnEndpoint {
            host: cdn_hosts
                .first()
                .ok_or_else(|| InstallationError::Other("No CDN hosts available".to_string()))?
                .clone(),
            path: plan.configs.cdn_path.clone(),
            product_path: None,
            scheme: None,
        };

        let config_key = hex::decode(&plan.configs.cdn_config).map_err(|e| {
            InstallationError::InvalidConfiguration(format!("Invalid CDN config hash: {e}"))
        })?;

        self.download_with_retry(
            &cdn_endpoint,
            ContentType::Config,
            &config_key,
            "CDN config",
        )
        .await
    }

    /// Download build config data from the CDN
    ///
    /// # Errors
    ///
    /// Returns an error if download fails or hash is invalid.
    async fn download_build_config(&self, plan: &InstallationPlan) -> Result<Vec<u8>> {
        // Use community CDN fallback for historic builds
        let cdn_hosts = self.get_cdn_hosts_with_fallback(plan);
        let cdn_endpoint = CdnEndpoint {
            host: cdn_hosts
                .first()
                .ok_or_else(|| InstallationError::Other("No CDN hosts available".to_string()))?
                .clone(),
            path: plan.configs.cdn_path.clone(),
            product_path: None,
            scheme: None,
        };

        let config_key = hex::decode(&plan.configs.build_config).map_err(|e| {
            InstallationError::InvalidConfiguration(format!("Invalid build config hash: {e}"))
        })?;

        self.download_with_retry(
            &cdn_endpoint,
            ContentType::Config,
            &config_key,
            "build config",
        )
        .await
    }

    /// Download and store CDN archive indices in batches to avoid memory issues
    ///
    /// Processes archive indices in batches with concurrent downloads to optimize
    /// memory usage while maintaining good throughput.
    ///
    /// # Errors
    ///
    /// Returns an error if downloads or file writes fail.
    async fn download_and_store_archive_indices_streaming(
        &self,
        plan: &InstallationPlan,
        target_dir: &Path,
    ) -> Result<()> {
        // Process in reasonable batches with good concurrency
        const BATCH_SIZE: usize = 20; // Moderate batch size
        const CONCURRENT_DOWNLOADS: usize = 8; // Reasonable concurrency limit

        println!("→ Downloading archive index files from CDN...");

        // First download and parse the CDN config to get archive index list
        let cdn_config_data = self.download_cdn_config(plan).await?;
        let cdn_config =
            FormatsCdnConfig::parse(std::io::Cursor::new(&cdn_config_data)).map_err(|e| {
                InstallationError::InvalidConfiguration(format!("Failed to parse CDN config: {e}"))
            })?;

        // Use community CDN fallback for historic builds
        let cdn_hosts = self.get_cdn_hosts_with_fallback(plan);
        let cdn_endpoint = CdnEndpoint {
            host: cdn_hosts
                .first()
                .ok_or_else(|| InstallationError::Other("No CDN hosts available".to_string()))?
                .clone(),
            path: plan.configs.cdn_path.clone(),
            product_path: None,
            scheme: None,
        };

        // For fresh installations, only download main archives (skip patch archives)
        let main_archives = cdn_config.archives();
        let patch_archives = cdn_config.patch_archives();

        println!(
            "→ Found {} main archives and {} patch archives",
            main_archives.len(),
            patch_archives.len()
        );

        // For fresh installations, we only need main archives
        // Patch archives are only needed when upgrading from previous builds
        let archives_to_download = main_archives;
        let archives_count = archives_to_download.len();

        println!(
            "→ Downloading {} main archive indices (skipping {} patch archives for fresh install)",
            archives_count,
            patch_archives.len()
        );

        let indices_dir = target_dir.join("Data").join("indices");
        fs::create_dir_all(&indices_dir).map_err(|e| {
            InstallationError::Other(format!("Failed to create indices directory: {e}"))
        })?;

        let mut processed = 0;
        for batch in archives_to_download.chunks(BATCH_SIZE) {
            // Check if all in this batch already exist
            let mut batch_to_download = Vec::new();
            for archive_info in batch {
                let index_path = indices_dir.join(format!("{}.index", archive_info.content_key));
                if !index_path.exists() {
                    batch_to_download.push(archive_info.clone());
                }
            }

            // Skip this batch if all files exist
            if batch_to_download.is_empty() {
                processed += batch.len();
                continue;
            }

            // Download this batch
            let cdn_client = &self.cdn_client;
            let endpoint = &cdn_endpoint;

            // Use into_iter() instead of iter() to avoid borrowing across await points
            let batch_indices: Vec<(String, Vec<u8>)> = stream::iter(batch_to_download.into_iter())
                .map(|archive_info| async move {
                    const MAX_RETRIES: usize = 3;
                    let archive_hash_str = &archive_info.content_key;

                    // Retry logic with exponential backoff
                    let mut retry_count = 0;
                    let mut delay = std::time::Duration::from_millis(500);

                    loop {
                        // Use the dedicated download_archive_index method which handles the .index suffix
                        match cdn_client
                            .download_archive_index(endpoint, archive_hash_str)
                            .await
                        {
                            Ok(index_data) => {
                                return Ok::<(String, Vec<u8>), InstallationError>((
                                    archive_hash_str.clone(),
                                    index_data,
                                ));
                            }
                            Err(e) => {
                                retry_count += 1;
                                if retry_count >= MAX_RETRIES {
                                    return Err(InstallationError::NetworkError(format!(
                                        "Failed to download archive index {archive_hash_str} after {MAX_RETRIES} retries: {e}"
                                    )));
                                }

                                // Log retry attempt
                                eprintln!(
                                    "  Retry {retry_count}/{MAX_RETRIES} for archive index {archive_hash_str} after error: {e}"
                                );

                                // Wait before retry with exponential backoff
                                time::sleep(delay).await;
                                delay *= 2; // Double the delay for next retry
                            }
                        }
                    }
                })
                .buffer_unordered(CONCURRENT_DOWNLOADS)
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .collect::<Result<Vec<_>>>()?;

            // Immediately write this batch to disk
            for (archive_hash, index_data) in batch_indices {
                let index_path = indices_dir.join(format!("{archive_hash}.index"));

                // Skip if already exists
                if index_path.exists() {
                    continue;
                }

                fs::write(&index_path, index_data).map_err(|e| {
                    InstallationError::Other(format!(
                        "Failed to write archive index {archive_hash}: {e}"
                    ))
                })?;
            }

            processed += batch.len();
            println!("  Downloaded and stored {processed}/{archives_count} archive indices");
        }

        println!("✓ Downloaded and stored {archives_count} archive indices");
        Ok(())
    }

    /// Get CDN hosts with community CDN fallback for historic builds
    ///
    /// Returns a list of CDN hosts prioritized by build type. Historic builds
    /// prioritize community mirrors while current builds use official CDN first.
    fn get_cdn_hosts_with_fallback(&self, plan: &InstallationPlan) -> Vec<String> {
        let mut cdn_hosts = Vec::new();

        // For historic builds, prioritize community CDN hosts
        if matches!(plan.build, crate::models::BuildSelection::Historic { .. }) {
            // Add community CDN hosts first for better historic build support
            cdn_hosts.push("cdn.arctium.tools".to_string());
            cdn_hosts.push("casc.wago.tools".to_string());
            cdn_hosts.push("tact.mirror.reliquaryhq.com".to_string());
        }

        // Add original CDN hosts from the plan
        cdn_hosts.extend(plan.configs.cdn_hosts.clone());

        // For current builds, also add community CDN as fallback
        if !matches!(plan.build, crate::models::BuildSelection::Historic { .. }) {
            cdn_hosts.push("cdn.arctium.tools".to_string());
        }

        cdn_hosts
    }

    /// Normalize install file paths for Battle.net compatibility
    ///
    /// Converts Windows backslashes to forward slashes and normalizes directory names
    /// on case-sensitive filesystems while preserving filename case.
    ///
    /// On case-sensitive filesystems (Linux, macOS), directory components are converted
    /// to uppercase because Blizzard's install manifests contain mixed-case directory paths
    /// (e.g., Utils/ and UTILS/) that should resolve to the same directory (UTILS/).
    /// However, filename case is preserved exactly as specified in the manifest to match
    /// Battle.net's behavior (e.g., BlizzardBrowser.exe, it.pak).
    ///
    /// # Arguments
    ///
    /// * `file_path` - Original file path from install manifest
    ///
    /// # Returns
    ///
    /// Normalized path string with uppercase directories and preserved filename case
    fn normalize_install_path(file_path: &str) -> String {
        // Convert Windows backslashes to forward slashes
        let path = file_path.replace('\\', "/");

        // On case-sensitive filesystems, normalize directory names to uppercase
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            // Split path into components
            let components: Vec<&str> = path.split('/').collect();

            // If there's more than one component, uppercase all but the last (filename)
            if components.len() > 1 {
                // Uppercase all directory components, preserve filename case
                let mut normalized = Vec::with_capacity(components.len());
                for (i, component) in components.iter().enumerate() {
                    if i < components.len() - 1 {
                        // Directory component - uppercase
                        normalized.push(component.to_uppercase());
                    } else {
                        // Filename component - preserve case
                        normalized.push(component.to_string());
                    }
                }
                normalized.join("/")
            } else {
                // Single component (no directories), preserve as-is
                path
            }
        }

        // On Windows (case-insensitive), preserve original case
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            path
        }
    }

    /// Download and extract files to the target directory
    ///
    /// Main extraction method that handles concurrent file downloads with resume support,
    /// progress tracking, and archive optimization.
    ///
    /// # Errors
    ///
    /// Returns an error if file extraction fails for any file.
    async fn download_and_extract_files(
        &mut self,
        plan: &InstallationPlan,
        install_files: &[cascette_formats::install::InstallFileEntry],
        encoding_file: &EncodingFile,
        archive_indices: &[(String, ArchiveIndex)],
    ) -> Result<()> {
        // Clone parameters to avoid borrowing across await points (fixes Send trait issues)
        let install_files = install_files.to_vec();
        let archive_indices = archive_indices.to_vec();

        // Initialize resume manager if not already present
        if self.resume_manager.is_none() {
            let mut manager = ResumeManager::new(&plan.target.directory);

            // Calculate plan hash for validation
            let plan_hash = format!("{}-{}", plan.product.code, plan.build.build_id());

            manager.initialize(
                plan_hash,
                install_files.len(),
                plan.build.build_id(),
                plan.product.code.clone(),
            )?;

            self.resume_manager = Some(manager);
        }

        // Get initial progress from resume manager
        let (already_completed, total, percentage) = self
            .resume_manager
            .as_ref()
            .and_then(ResumeManager::get_progress)
            .unwrap_or((0, install_files.len(), 0.0));

        if already_completed > 0 {
            println!(
                "→ Resuming download: {already_completed}/{total} files already completed ({percentage:.1}%)"
            );
        } else {
            println!(
                "→ Downloading and extracting {} files...",
                install_files.len()
            );
        }

        // Use community CDN fallback for historic builds
        let cdn_hosts = self.get_cdn_hosts_with_fallback(plan);
        let cdn_endpoint = CdnEndpoint {
            host: cdn_hosts
                .first()
                .ok_or_else(|| InstallationError::Other("No CDN hosts available".to_string()))?
                .clone(),
            path: plan.configs.cdn_path.clone(),
            product_path: None,
            scheme: None,
        };

        // Analyze archive locations for block mapping optimization
        let mut block_map = ArchiveBlockMap::with_max_merge_gap(1024 * 1024); // 1MB gap
        let mut file_archive_mappings = Vec::new();

        // Build mapping of files to archive locations
        for file_entry in &install_files {
            let content_key = &file_entry.content_key;
            let encoding_keys = encoding_file.find_all_encodings(content_key);

            // Try to find the file in archives
            for encoding_key in &encoding_keys {
                let encoding_key_bytes = encoding_key.as_bytes();

                for (hash, cdn_index) in &archive_indices {
                    if let Some(entry) = cdn_index.find_entry(encoding_key_bytes) {
                        file_archive_mappings.push((
                            file_entry.path.clone(),
                            hash.clone(),
                            entry.clone(),
                        ));
                        break;
                    }
                }
            }
        }

        // Analyze the file locations to optimize download patterns
        block_map.analyze_install_files(&file_archive_mappings);

        let stats = block_map.get_statistics();
        if stats.savings_percent > 0.0 {
            println!(
                "→ Archive optimization: {:.1}% bandwidth reduction possible",
                stats.savings_percent
            );
        }

        // Prepare for concurrent processing
        let semaphore = Arc::new(Semaphore::new(4)); // Limit to 4 concurrent downloads
        let success_count = Arc::new(Mutex::new(already_completed));
        let error_count = Arc::new(Mutex::new(0usize));
        let skipped_count = Arc::new(Mutex::new(0usize));

        // Wrap shared state in Arc<Mutex> for thread-safe access
        let resume_manager = Arc::new(Mutex::new(self.resume_manager.take()));
        let progress_callback = Arc::new(Mutex::new(self.progress_callback.take()));
        let archive_manager = self.archive_manager.clone();
        let block_map = Arc::new(block_map);

        // Prepare tasks for concurrent execution
        let mut tasks = Vec::new();

        for (i, file_entry) in install_files.iter().enumerate() {
            // Check if this file was already completed (before spawning task)
            let should_skip = {
                let manager_guard = resume_manager.lock().await;
                if let Some(ref manager) = *manager_guard {
                    manager.should_skip(&file_entry.path)
                } else {
                    false
                }
            };

            if should_skip {
                let mut count = skipped_count.lock().await;
                *count += 1;
                if *count <= 10 {
                    println!("  ⏩ Skipping already completed: {}", file_entry.path);
                } else if *count == 11 {
                    println!("  ⏩ Skipping remaining completed files...");
                }
                continue;
            }

            // Clone necessary data for the async task
            let semaphore = semaphore.clone();
            let cdn_client = self.cdn_client.clone();
            let cdn_endpoint = cdn_endpoint.clone();
            let file_entry = file_entry.clone();
            let encoding_file = encoding_file.clone();
            let archive_indices = archive_indices.clone();
            let target_dir = plan.target.directory.clone();
            let success_count = success_count.clone();
            let error_count = error_count.clone();
            let resume_manager = resume_manager.clone();
            let progress_callback = progress_callback.clone();
            let archive_manager = archive_manager.clone();
            let block_map = block_map.clone();

            // Create concurrent task
            let task = tokio::spawn(async move {
                // Acquire semaphore permit to limit concurrency
                let _permit = match semaphore.acquire().await {
                    Ok(permit) => permit,
                    Err(e) => {
                        // Semaphore closed - increment error count and exit task
                        let mut count = error_count.lock().await;
                        *count += 1;
                        eprintln!(
                            "  ✗ Failed to acquire semaphore for {}: {}",
                            file_entry.path, e
                        );

                        // Notify progress callback of error
                        {
                            let mut callback = progress_callback.lock().await;
                            if let Some(ref mut cb) = *callback {
                                cb.on_error(&format!(
                                    "Semaphore error for {}: {}",
                                    file_entry.path, e
                                ));
                            }
                        }
                        return;
                    }
                };

                // Notify progress callback of file start
                {
                    let mut callback = progress_callback.lock().await;
                    if let Some(ref mut cb) = *callback {
                        cb.on_file_start(&format!("file_{i}"), u64::from(file_entry.file_size));
                    }
                }

                // Perform the actual download and extraction
                let result = Self::download_and_extract_file_static(
                    &cdn_client,
                    &cdn_endpoint,
                    &file_entry,
                    &encoding_file,
                    &archive_indices,
                    &target_dir,
                    archive_manager.as_ref(),
                    &block_map,
                )
                .await;

                // Process result
                match result {
                    Ok(extracted_path) => {
                        let mut count = success_count.lock().await;
                        *count += 1;
                        println!("  ✓ {extracted_path}");

                        // Mark file as completed in resume state
                        {
                            let mut manager_guard = resume_manager.lock().await;
                            if let Some(ref mut manager) = *manager_guard {
                                if let Err(e) = manager.mark_completed(file_entry.path.clone()) {
                                    eprintln!("Failed to mark file as completed: {e}");
                                }
                            }
                        }

                        // Notify progress callback of completion
                        {
                            let mut callback = progress_callback.lock().await;
                            if let Some(ref mut cb) = *callback {
                                cb.on_file_complete(&format!("file_{i}"));
                            }
                        }
                    }
                    Err(e) => {
                        let mut count = error_count.lock().await;
                        *count += 1;
                        println!("  ✗ Failed to extract file {}: {}", file_entry.path, e);

                        // Notify progress callback of error
                        {
                            let mut callback = progress_callback.lock().await;
                            if let Some(ref mut cb) = *callback {
                                cb.on_error(&format!("file_{i}: {e}"));
                            }
                        }

                        // Save state even on error so we don't lose progress
                        {
                            let manager_guard = resume_manager.lock().await;
                            if let Some(ref manager) = *manager_guard {
                                if let Err(e) = manager.save() {
                                    eprintln!("Failed to save resume state: {e}");
                                }
                            }
                        }
                    }
                }
            });

            tasks.push(task);
        }

        // Wait for all tasks to complete
        for task in tasks {
            let _ = task.await; // Ignore JoinError, errors are tracked in counts
        }

        // Restore shared state back to self
        self.resume_manager = resume_manager.lock().await.take();
        self.progress_callback = progress_callback.lock().await.take();

        // Final save of resume state
        if let Some(ref manager) = self.resume_manager {
            manager.save()?;
        }

        let final_success_count = *success_count.lock().await;
        let final_error_count = *error_count.lock().await;

        println!(
            "→ Download complete: {} successful, {} failed",
            final_success_count - already_completed,
            final_error_count
        );

        let final_skipped_count = *skipped_count.lock().await;
        if final_skipped_count > 0 {
            println!("  {final_skipped_count} files were already completed from previous run");
        }

        if final_error_count > 0 {
            return Err(InstallationError::Other(format!(
                "Failed to download {final_error_count} files. Installation state saved - run again to resume."
            )));
        }

        // Clear resume state after successful completion
        if let Some(ref manager) = self.resume_manager {
            manager.clear()?;
        }

        // Clear progress state if using persistent progress
        if let Some(ref callback) = self.progress_callback {
            callback.on_completion_cleanup();
        }

        Ok(())
    }

    /// Download and extract a single file from archives
    ///
    /// Static method for concurrent use that looks up the file in archive indices,
    /// downloads the required archive data, and extracts the file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be found or extraction fails.
    #[allow(clippy::too_many_arguments)]
    async fn download_and_extract_file_static(
        cdn_client: &Arc<CdnClient>,
        cdn_endpoint: &CdnEndpoint,
        file_entry: &cascette_formats::install::InstallFileEntry,
        encoding_file: &EncodingFile,
        archive_indices: &[(String, ArchiveIndex)],
        target_dir: &Path,
        archive_manager: Option<&Arc<Mutex<ArchiveManager>>>,
        block_map: &Arc<ArchiveBlockMap>,
    ) -> Result<String> {
        // Step 1: Look up all encoding keys from content key
        let content_key = &file_entry.content_key;
        let encoding_keys = encoding_file.find_all_encodings(content_key);

        if encoding_keys.is_empty() {
            return Err(InstallationError::Other(format!(
                "Could not find any encoding keys for content key: {}",
                content_key.to_hex()
            )));
        }

        // Step 2: Get file path from install entry (already available)
        let file_path = &file_entry.path;

        // Step 3: Find the file in CDN archive indices - try all encoding keys
        let mut archive_entry = None;
        let mut archive_hash = None;
        let mut found_encoding_key = None;

        // Debug logging can be enabled via environment variable
        if std::env::var("CASCETTE_DEBUG").is_ok() {
            eprintln!(
                "DEBUG: Looking for {} with {} encoding key(s)",
                file_path,
                encoding_keys.len()
            );
            for ekey in &encoding_keys {
                eprintln!("  - Encoding key: {}", hex::encode(ekey.as_bytes()));
            }
        }

        // Try each encoding key until we find one in the archives
        for encoding_key in &encoding_keys {
            let encoding_key_bytes = encoding_key.as_bytes();

            for (hash, cdn_index) in archive_indices {
                if let Some(entry) = cdn_index.find_entry(encoding_key_bytes) {
                    archive_entry = Some(entry.clone());
                    archive_hash = Some(hash.clone());
                    found_encoding_key = Some(*encoding_key);
                    break;
                }
            }

            if archive_entry.is_some() {
                break;
            }
        }

        // Check if we found the file in archives
        if let (Some(entry), Some(archive), Some(found_ekey)) =
            (archive_entry, archive_hash, found_encoding_key)
        {
            // File found in archive - extract it
            // Normalize path for Battle.net compatibility (T083a)
            let normalized_path = Self::normalize_install_path(file_path);
            let output_path = target_dir.join(&normalized_path);

            Self::extract_file_from_archive_static(
                cdn_client,
                cdn_endpoint,
                &archive,
                &entry,
                &found_ekey,
                file_path,
                &output_path,
                archive_manager,
                block_map,
            )
            .await?;
            Ok(normalized_path)
        } else {
            // File not in archives - try downloading as loose file
            if std::env::var("CASCETTE_DEBUG").is_ok() {
                eprintln!(
                    "DEBUG: File {file_path} not found in archives, trying loose file download"
                );
            }

            // TACTSharp fallback: download directly from CDN using encoding key
            // Try each encoding key until one works
            let mut last_error = None;
            for encoding_key in &encoding_keys {
                // Normalize path for Battle.net compatibility (T083a)
                let normalized_path = Self::normalize_install_path(file_path);
                let output_path = target_dir.join(&normalized_path);

                match Self::download_loose_file_static(
                    cdn_client,
                    cdn_endpoint,
                    encoding_key,
                    file_path,
                    &output_path,
                )
                .await
                {
                    Ok(()) => return Ok(normalized_path.clone()),
                    Err(e) => last_error = Some(e),
                }
            }

            // If all encoding keys failed, return the last error
            match last_error {
                Some(e) => Err(e),
                None => Err(InstallationError::Other(format!(
                    "No encoding keys available for file {file_path}"
                ))),
            }
        }
    }

    /// Extract a file from an archive
    ///
    /// Downloads the required byte range from the archive, decompresses the BLTE data,
    /// and writes the file atomically to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if download, decompression, or file write fails.
    #[allow(clippy::too_many_arguments)]
    async fn extract_file_from_archive_static(
        cdn_client: &Arc<CdnClient>,
        cdn_endpoint: &CdnEndpoint,
        archive: &str,
        entry: &IndexEntry,
        found_ekey: &EncodingKey,
        file_path: &str,
        output_path: &Path,
        archive_manager: Option<&Arc<Mutex<ArchiveManager>>>,
        _block_map: &Arc<ArchiveBlockMap>,
    ) -> Result<()> {
        // Debug: Log the encoding key we found
        if std::env::var("CASCETTE_DEBUG").is_ok() {
            eprintln!(
                "DEBUG: Found encoding key {} in archive {}",
                hex::encode(found_ekey.as_bytes()),
                archive
            );
        }

        // Download the archive data file that contains our file
        // The archive hash is a hex string that needs to be decoded to bytes
        let archive_key = hex::decode(archive).map_err(|e| {
            InstallationError::InvalidConfiguration(format!("Invalid archive hash {archive}: {e}"))
        })?;

        // Try to get the data from local archive cache first, then from CDN if needed
        let archive_data = if archive_manager.is_some() {
            // Use a dedicated range cache directory
            let cache_base = dirs::cache_dir()
                .ok_or_else(|| {
                    InstallationError::NetworkError(
                        "Could not determine cache directory".to_string(),
                    )
                })?
                .join("cascette")
                .join("local_archives")
                .join("range_cache");

            // Create a cache file path based on archive hash and range
            let cache_dir = cache_base.join(&archive[0..2]);
            let cache_file = cache_dir.join(format!("{}_{}_{}", archive, entry.offset, entry.size));

            // Try to read from cache file first
            if cache_file.exists() {
                match fs::read(&cache_file) {
                    Ok(data) => {
                        if std::env::var("CASCETTE_DEBUG").is_ok() {
                            eprintln!(
                                "DEBUG: Using cached range from {} for offset {} size {}",
                                archive, entry.offset, entry.size
                            );
                        }
                        data
                    }
                    Err(e) => {
                        // Cache read failed, download from CDN
                        eprintln!(
                            "Warning: Failed to read cache file {}: {}",
                            cache_file.display(),
                            e
                        );
                        let data = cdn_client
                            .download_range(
                                cdn_endpoint,
                                ContentType::Data,
                                &archive_key,
                                entry.offset,
                                u64::from(entry.size),
                            )
                            .await
                            .map_err(|e| {
                                InstallationError::NetworkError(format!(
                                    "Failed to download archive range from {} (offset: {}, size: {}): {}",
                                    archive, entry.offset, entry.size, e
                                ))
                            })?;

                        // Try to cache for future use
                        if let Err(e) = fs::create_dir_all(&cache_dir) {
                            eprintln!("Warning: Failed to create cache directory: {e}");
                        } else if let Err(e) = fs::write(&cache_file, &data) {
                            eprintln!("Warning: Failed to write cache file: {e}");
                        } else if std::env::var("CASCETTE_DEBUG").is_ok() {
                            eprintln!(
                                "DEBUG: Cached range to {} for offset {} size {}",
                                cache_file.display(),
                                entry.offset,
                                entry.size
                            );
                        }

                        data
                    }
                }
            } else {
                // Not in cache, download from CDN
                let data = cdn_client
                    .download_range(
                        cdn_endpoint,
                        ContentType::Data,
                        &archive_key,
                        entry.offset,
                        u64::from(entry.size),
                    )
                    .await
                    .map_err(|e| {
                        InstallationError::NetworkError(format!(
                            "Failed to download archive range from {} (offset: {}, size: {}): {}",
                            archive, entry.offset, entry.size, e
                        ))
                    })?;

                // Try to cache for future use
                if let Err(e) = fs::create_dir_all(&cache_dir) {
                    eprintln!("Warning: Failed to create cache directory: {e}");
                } else if let Err(e) = fs::write(&cache_file, &data) {
                    eprintln!("Warning: Failed to write cache file: {e}");
                } else if std::env::var("CASCETTE_DEBUG").is_ok() {
                    eprintln!(
                        "DEBUG: Cached range to {} for offset {} size {}",
                        cache_file.display(),
                        entry.offset,
                        entry.size
                    );
                }

                data
            }
        } else {
            // No archive manager, just download from CDN
            cdn_client
                .download_range(
                    cdn_endpoint,
                    ContentType::Data,
                    &archive_key,
                    entry.offset,
                    u64::from(entry.size),
                )
                .await
                .map_err(|e| {
                    InstallationError::NetworkError(format!(
                        "Failed to download archive range from {} (offset: {}, size: {}): {}",
                        archive, entry.offset, entry.size, e
                    ))
                })?
        };

        // Step 5: We now have exactly the data we need (from cache or CDN)
        let compressed_data = &archive_data;

        // Step 6: BLTE decompress the data
        let blte_file = BlteFile::parse(compressed_data).map_err(|e| {
            InstallationError::InvalidConfiguration(format!(
                "Failed to parse BLTE data for {file_path}: {e}"
            ))
        })?;

        let file_data = blte_file.decompress().map_err(|e| {
            InstallationError::InvalidConfiguration(format!(
                "Failed to decompress BLTE data for {file_path}: {e}"
            ))
        })?;

        // Create parent directories
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                InstallationError::Other(format!(
                    "Failed to create directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        // Write the file atomically using the atomic writer module
        crate::atomic_writer::atomic_write(output_path, &file_data).map_err(|e| {
            InstallationError::Other(format!(
                "Failed to atomically write file {}: {}",
                output_path.display(),
                e
            ))
        })?;

        Ok(())
    }

    /// Download a file directly from CDN as a loose file
    ///
    /// Used for files not found in archives. Downloads directly using the encoding key
    /// and decompresses the BLTE data.
    ///
    /// # Errors
    ///
    /// Returns an error if download, decompression, or file write fails.
    async fn download_loose_file_static(
        cdn_client: &Arc<CdnClient>,
        cdn_endpoint: &CdnEndpoint,
        encoding_key: &EncodingKey,
        file_path: &str,
        output_path: &Path,
    ) -> Result<()> {
        // Debug output
        if std::env::var("CASCETTE_DEBUG").is_ok() {
            eprintln!(
                "DEBUG: Attempting loose file download for {} with encoding key {}",
                file_path,
                hex::encode(encoding_key.as_bytes())
            );
        }

        // Download directly from CDN using encoding key
        let encoding_key_bytes = encoding_key.as_bytes();
        let file_data = cdn_client
            .download(cdn_endpoint, ContentType::Data, encoding_key_bytes)
            .await
            .map_err(|e| {
                InstallationError::NetworkError(format!(
                    "Failed to download loose file {file_path}: {e}"
                ))
            })?;

        // Decompress if BLTE encoded
        let blte_file = cascette_formats::blte::BlteFile::parse(&file_data).map_err(|e| {
            InstallationError::InvalidConfiguration(format!(
                "Failed to parse BLTE data for {file_path}: {e}"
            ))
        })?;

        let decompressed_data = blte_file.decompress().map_err(|e| {
            InstallationError::InvalidConfiguration(format!(
                "Failed to decompress BLTE data for {file_path}: {e}"
            ))
        })?;

        // Create parent directories
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                InstallationError::Other(format!(
                    "Failed to create directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        // Write the file atomically using the atomic writer module
        crate::atomic_writer::atomic_write(output_path, &decompressed_data).map_err(|e| {
            InstallationError::Other(format!(
                "Failed to atomically write file {}: {}",
                output_path.display(),
                e
            ))
        })?;

        if std::env::var("CASCETTE_DEBUG").is_ok() {
            eprintln!(
                "DEBUG: Successfully downloaded loose file {} to {}",
                file_path,
                output_path.display()
            );
        }

        Ok(())
    }
}

/// Console progress callback for basic progress reporting
///
/// Simple implementation that prints progress updates to stdout.
pub struct ConsoleProgressCallback;

impl ProgressCallback for ConsoleProgressCallback {
    fn on_file_start(&mut self, path: &str, size: u64) {
        println!(
            "  → Starting download: {} ({:.1} KB)",
            path,
            size as f64 / 1024.0
        );
    }

    fn on_progress(&mut self, downloaded: u64, total: u64) {
        let percent = if total > 0 {
            (downloaded as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        println!("    Progress: {percent:.1}%");
    }

    fn on_file_complete(&mut self, path: &str) {
        println!("  ✓ Completed: {path}");
    }

    fn on_error(&mut self, error: &str) {
        println!("  ✗ Error: {error}");
    }
}
