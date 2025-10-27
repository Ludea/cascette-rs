//! Metadata resolver for product installation using cascette-protocol

use crate::builds::manager::BuildManager;
use crate::{
    error::{InstallationError, Result},
    models::{
        ArchiveIndexInfo, ArchiveInfo, ArchiveSet, BuildSelection, ConfigurationSet,
        DownloadManifest, EncodingManifest, InstallManifest, ManifestSet, ProductInfo,
        RootManifest, RootVersion,
    },
};
use cascette_crypto::ContentKey;
use cascette_formats::CascFormat;
use cascette_formats::blte::BlteFile;
use cascette_formats::config::{BuildConfig, CdnConfig};
use cascette_formats::encoding::EncodingFile;
use cascette_formats::install::InstallManifest as FormatsInstallManifest;
use cascette_protocol::{CdnClient, CdnEndpoint, ClientConfig, ContentType, RibbitTactClient};
use chrono::Utc;
use std::path::Path;
use std::sync::Arc;

/// Resolves product metadata from NGDP
pub struct MetadataResolver {
    product_code: String,
    build_id: Option<u32>,
    client: Arc<RibbitTactClient>,
    cdn_client: Arc<CdnClient>,
    build_manager: Option<BuildManager>,
    debug_mode: bool,
}

impl MetadataResolver {
    /// Create a new resolver
    pub fn new(product_code: String, build_id: Option<u32>) -> Result<Self> {
        // Initialize protocol client with disk cache
        let cache_dir = dirs::cache_dir()
            .ok_or_else(|| {
                InstallationError::NetworkError("Could not determine cache directory".to_string())
            })?
            .join("cascette");

        // Ensure cache directory exists
        std::fs::create_dir_all(&cache_dir).map_err(|e| {
            InstallationError::NetworkError(format!("Failed to create cache directory: {e}"))
        })?;

        let mut config = ClientConfig::default();
        config.cache_config.cache_dir = Some(cache_dir);

        let client = Arc::new(
            RibbitTactClient::new(config)
                .map_err(|e| InstallationError::NetworkError(e.to_string()))?,
        );

        // Initialize CDN client with shared cache
        let cdn_client = Arc::new(
            CdnClient::new(
                client.cache().clone(),
                cascette_protocol::CdnConfig::default(),
            )
            .map_err(|e| InstallationError::NetworkError(e.to_string()))?,
        );

        Ok(Self {
            product_code,
            build_id,
            client,
            cdn_client,
            build_manager: None,
            debug_mode: std::env::var("CASCETTE_DEBUG").is_ok(),
        })
    }

    /// Set the build manager for accessing imported builds
    #[must_use]
    pub fn with_build_manager(mut self, data_dir: &Path) -> Self {
        self.build_manager = Some(BuildManager::new(data_dir));
        self
    }

    /// Resolve product information
    pub async fn resolve_product(&self) -> Result<ProductInfo> {
        // Query product information from NGDP
        let endpoint = format!("v1/products/{}/versions", self.product_code);
        let versions = self.client.query(&endpoint).await.map_err(|e| {
            InstallationError::NetworkError(format!("Failed to query versions: {e}"))
        })?;

        if versions.rows().is_empty() {
            return Err(InstallationError::ProductNotFound(
                self.product_code.clone(),
            ));
        }

        Ok(ProductInfo {
            code: self.product_code.clone(),
            name: self.product_code.clone(), // Display name can be resolved at UI layer
            region: "us".to_string(),
            channel: None,
        })
    }

    /// Resolve build information
    pub async fn resolve_build(&self) -> Result<BuildSelection> {
        let endpoint = format!("v1/products/{}/versions", self.product_code);
        if let Some(build_id) = self.build_id {
            // First try to find the build in NGDP
            let versions = self.client.query(&endpoint).await.map_err(|e| {
                InstallationError::NetworkError(format!("Failed to query versions: {e}"))
            })?;

            // Check if build exists in NGDP results
            let found_in_ngdp = versions.rows().iter().any(|row| {
                row.get_by_name("BuildId", versions.schema())
                    .and_then(|v| {
                        // Try as decimal first
                        if let Some(dec) = v.as_dec() {
                            Some(dec as u32)
                        } else if let Some(s) = v.as_string() {
                            s.parse::<u32>().ok()
                        } else {
                            None
                        }
                    })
                    .is_some_and(|id| id == build_id)
            });

            if found_in_ngdp {
                // Build is available in NGDP, use it
                let version_row = versions
                    .rows()
                    .iter()
                    .find(|row| {
                        row.get_by_name("BuildId", versions.schema())
                            .and_then(|v| {
                                if let Some(dec) = v.as_dec() {
                                    Some(dec as u32)
                                } else if let Some(s) = v.as_string() {
                                    s.parse::<u32>().ok()
                                } else {
                                    None
                                }
                            })
                            .is_some_and(|id| id == build_id)
                    })
                    .ok_or(InstallationError::BuildNotFound(build_id))?;

                let version = version_row
                    .get_by_name("VersionsName", versions.schema())
                    .and_then(|v| v.as_string())
                    .unwrap_or("Unknown");

                Ok(BuildSelection::Latest {
                    version: version.to_string(),
                    build_id,
                    discovered_at: Utc::now(),
                })
            } else {
                // Build not in NGDP, check locally imported builds
                if let Some(ref build_manager) = self.build_manager {
                    if build_manager.build_exists(&self.product_code, build_id) {
                        // Load the build metadata
                        let metadata = build_manager
                            .load_build(&self.product_code, build_id)
                            .map_err(|e| {
                                InstallationError::Other(format!(
                                    "Failed to load build metadata: {e}"
                                ))
                            })?;

                        Ok(BuildSelection::Historic {
                            version: metadata.build.version_build.clone(),
                            build_id,
                            source: match metadata.meta.source {
                                crate::builds::metadata::DataSource::Wago { .. } => {
                                    "wago.tools".to_string()
                                }
                                _ => "imported".to_string(),
                            },
                            imported_at: metadata.meta.captured_at,
                        })
                    } else {
                        // Build not found locally either
                        Err(InstallationError::BuildNotFound(build_id))
                    }
                } else {
                    // No build manager configured, can't check local builds
                    Err(InstallationError::BuildNotFound(build_id))
                }
            }
        } else {
            // Get latest build from NGDP
            let endpoint = format!("v1/products/{}/versions", self.product_code);
            let versions = self.client.query(&endpoint).await.map_err(|e| {
                InstallationError::NetworkError(format!("Failed to query versions: {e}"))
            })?;

            let latest = versions
                .rows()
                .first()
                .ok_or_else(|| InstallationError::ProductNotFound(self.product_code.clone()))?;

            let version = latest
                .get_by_name("VersionsName", versions.schema())
                .and_then(|v| v.as_string())
                .unwrap_or("Unknown");

            // BuildId can be decimal or string
            let build_id = if let Some(value) = latest.get_by_name("BuildId", versions.schema()) {
                match value.as_dec() {
                    Some(d) => d as u32,
                    None => value
                        .as_string()
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(0),
                }
            } else {
                0
            };

            Ok(BuildSelection::Latest {
                version: version.to_string(),
                build_id,
                discovered_at: Utc::now(),
            })
        }
    }

    /// Resolve configuration data
    pub async fn resolve_configs(&self) -> Result<ConfigurationSet> {
        // If we have a specific build ID, it MUST be imported locally
        if let Some(build_id) = self.build_id {
            // Build manager is required for historic builds
            let build_manager = self.build_manager.as_ref().ok_or_else(|| {
                InstallationError::Other(
                    "Build manager not initialized for historic build".to_string(),
                )
            })?;

            // Check if the build exists locally
            if !build_manager.build_exists(&self.product_code, build_id) {
                return Err(InstallationError::Other(format!(
                    "Build {} not found. Please import historic builds first using:\n  \
                     cascette import {} --build {}",
                    build_id, self.product_code, build_id
                )));
            }

            // Load the build metadata for historic build
            let metadata = build_manager
                .load_build(&self.product_code, build_id)
                .map_err(|e| {
                    InstallationError::Other(format!("Failed to load build metadata: {e}"))
                })?;

            // Get CDN info (we'll still need to query for this)
            let cdn_info = self.get_cdn_info().await?;

            // Return config set from imported metadata
            return Ok(ConfigurationSet {
                build_config: metadata.configs.build_config,
                cdn_config: metadata.configs.cdn_config,
                product_config: metadata.configs.product_config,
                install_key: None, // Not available in imported metadata
                cdn_path: cdn_info.path,
                cdn_hosts: cdn_info.hosts,
                tags: Self::detect_platform_tags(),
            });
        }

        // Query version information to get config hashes from NGDP
        let endpoint = format!("v1/products/{}/versions", self.product_code);
        let versions = self.client.query(&endpoint).await.map_err(|e| {
            InstallationError::NetworkError(format!("Failed to query versions: {e}"))
        })?;

        // Debug: Check if we have any rows
        if versions.rows().is_empty() {
            return Err(InstallationError::NetworkError(format!(
                "No version data returned for product '{}'",
                self.product_code
            )));
        }

        let version_row = if let Some(build_id) = self.build_id {
            // Find specific build
            versions
                .rows()
                .iter()
                .find(|row| {
                    // BuildId can be either a decimal value or a string
                    let row_build_id =
                        row.get_by_name("BuildId", versions.schema()).and_then(|v| {
                            // Try as decimal first
                            if let Some(dec) = v.as_dec() {
                                Some(dec as u32)
                            } else if let Some(s) = v.as_string() {
                                s.parse::<u32>().ok()
                            } else {
                                None
                            }
                        });

                    row_build_id.is_some_and(|id| id == build_id)
                })
                .ok_or(InstallationError::BuildNotFound(build_id))?
        } else {
            // Use latest
            versions
                .rows()
                .first()
                .ok_or_else(|| InstallationError::ProductNotFound(self.product_code.clone()))?
        };

        // Extract config hashes - field names are CamelCase in NGDP
        let build_config_value = version_row
            .get_by_name("BuildConfig", versions.schema())
            .ok_or_else(|| {
                // Debug: List available fields
                let field_names: Vec<String> = versions
                    .schema()
                    .fields()
                    .iter()
                    .map(|f| f.name.clone())
                    .collect();
                InstallationError::InvalidConfiguration(format!(
                    "Missing BuildConfig field. Available fields: {field_names:?}"
                ))
            })?;

        // Handle both string and hex values
        let build_config_hash = match build_config_value.as_string() {
            Some(s) => s.to_string(),
            None => match build_config_value.as_hex() {
                Some(h) => hex::encode(h),
                None => {
                    return Err(InstallationError::InvalidConfiguration(format!(
                        "BuildConfig field has unexpected type: {build_config_value:?}"
                    )));
                }
            },
        };

        let cdn_config_value = version_row
            .get_by_name("CDNConfig", versions.schema())
            .ok_or_else(|| {
                InstallationError::InvalidConfiguration("Missing CDNConfig field".to_string())
            })?;

        let cdn_config_hash = match cdn_config_value.as_string() {
            Some(s) => s.to_string(),
            None => match cdn_config_value.as_hex() {
                Some(h) => hex::encode(h),
                None => {
                    return Err(InstallationError::InvalidConfiguration(format!(
                        "CDNConfig field has unexpected type: {cdn_config_value:?}"
                    )));
                }
            },
        };

        // Get CDN information
        let cdn_info = self.get_cdn_info().await?;

        // Extract install key if present
        let install_key = version_row
            .get_by_name("InstallKey", versions.schema())
            .and_then(|v| match v.as_string() {
                Some(s) => Some(s.to_string()),
                None => v.as_hex().map(hex::encode),
            });

        // Extract product config if present (only available for some products/builds)
        let product_config = version_row
            .get_by_name("ProductConfig", versions.schema())
            .and_then(|v| match v.as_string() {
                Some(s) => Some(s.to_string()),
                None => v.as_hex().map(hex::encode),
            });

        // Extract tags based on current platform
        let tags = Self::detect_platform_tags();

        Ok(ConfigurationSet {
            build_config: build_config_hash,
            cdn_config: cdn_config_hash,
            product_config,
            install_key,
            cdn_path: cdn_info.path,
            cdn_hosts: cdn_info.hosts,
            tags,
        })
    }

    /// Resolve manifest data by downloading and parsing actual configs
    pub async fn resolve_manifests(&self, configs: &ConfigurationSet) -> Result<ManifestSet> {
        if self.debug_mode {
            eprintln!(
                "\n=== Starting manifest resolution for {} ===",
                self.product_code
            );
            eprintln!("Build config hash: {}", configs.build_config);
            eprintln!("CDN config hash: {}", configs.cdn_config);
            eprintln!("CDN hosts: {:?}", configs.cdn_hosts);
            eprintln!("CDN path: {}", configs.cdn_path);
        }

        // Historic builds still need to download configs from CDN using the hashes
        // The imported data only contains the hashes, not the actual config content

        // Create CDN endpoint from the configuration
        let endpoint = CdnEndpoint {
            host: configs
                .cdn_hosts
                .first()
                .ok_or_else(|| {
                    InstallationError::NetworkError("No CDN hosts available".to_string())
                })?
                .clone(),
            path: configs.cdn_path.clone(),
            product_path: None, // Will be set based on product
            scheme: None,
        };

        // Download and parse BuildConfig with fallback to community mirrors
        if self.debug_mode {
            eprintln!("\n--- Downloading BuildConfig ---");
        }
        let build_config = self
            .download_build_config_with_fallback(
                &endpoint,
                &configs.build_config,
                &configs.cdn_hosts,
            )
            .await?;

        if self.debug_mode {
            eprintln!("BuildConfig downloaded successfully");
            eprintln!("  Root: {:?}", build_config.root());
            eprintln!("  Encoding: {:?}", build_config.encoding());
            eprintln!("  Install entries: {}", build_config.install().len());
            eprintln!("  Download entries: {}", build_config.download().len());
        }

        // Note: CDNConfig is downloaded separately in resolve_archives() to extract archive info

        // Extract manifest information from BuildConfig

        let encoding_info = build_config.encoding().ok_or_else(|| {
            InstallationError::InvalidConfiguration("No encoding info in BuildConfig".to_string())
        })?;

        let install_infos = build_config.install();
        let install_info = install_infos.first().ok_or_else(|| {
            InstallationError::InvalidConfiguration("No install info in BuildConfig".to_string())
        })?;

        // Some products (agent, bna) don't have root manifests
        let root_content_key = build_config.root();

        if self.debug_mode && root_content_key.is_none() {
            eprintln!(
                "  Note: No root manifest found for {} (this is normal for utility products)",
                self.product_code
            );
        }

        // Convert hex strings to byte arrays (using first 16 bytes of MD5)
        // IMPORTANT: Use encoding_key (CDN hash) not content_key (lookup hash)
        let encoding_cdn_key = encoding_info.encoding_key.as_ref().ok_or_else(|| {
            InstallationError::InvalidConfiguration("No encoding key in BuildConfig".to_string())
        })?;
        let encoding_key = hex::decode(encoding_cdn_key).map_err(|e| {
            InstallationError::InvalidConfiguration(format!("Invalid encoding key: {e}"))
        })?;
        let encoding_key_bytes: [u8; 16] = encoding_key[..16].try_into().map_err(|_| {
            InstallationError::InvalidConfiguration("Encoding key wrong size".to_string())
        })?;

        let install_content_key = hex::decode(&install_info.content_key).map_err(|e| {
            InstallationError::InvalidConfiguration(format!("Invalid install key: {e}"))
        })?;
        let _install_key_bytes: [u8; 16] = install_content_key[..16].try_into().map_err(|_| {
            InstallationError::InvalidConfiguration("Install key wrong size".to_string())
        })?;

        // Only parse root key if we have one
        let _root_key_bytes: Option<[u8; 16]> = if let Some(ref root_key_str) = root_content_key {
            let root_key = hex::decode(root_key_str).map_err(|e| {
                InstallationError::InvalidConfiguration(format!("Invalid root key: {e}"))
            })?;
            let key_array: [u8; 16] = root_key[..16].try_into().map_err(|_| {
                InstallationError::InvalidConfiguration("Root key wrong size".to_string())
            })?;
            Some(key_array)
        } else {
            None
        };

        // Download and parse the actual encoding manifest to get metadata
        // Try multiple CDN hosts including community mirrors for historic data
        if self.debug_mode {
            eprintln!("\n--- Downloading Encoding Manifest ---");
            eprintln!("Encoding key: {}", hex::encode(&encoding_key));
        }

        let (encoding_manifest, encoding_file_opt) = match self
            .download_encoding_manifest_with_fallback(&endpoint, &encoding_key, &configs.cdn_hosts)
            .await
        {
            Ok(decoded_data) => {
                if self.debug_mode {
                    eprintln!("Downloaded encoding manifest successfully");
                    eprintln!("  Size: {} bytes", decoded_data.len());
                    eprintln!(
                        "  First 16 bytes: {:02x?}",
                        &decoded_data[..16.min(decoded_data.len())]
                    );
                    eprintln!("\n--- Parsing Encoding Manifest ---");
                    eprintln!("Calling EncodingFile::parse()...");
                }

                // Parse the actual encoding manifest using cascette-formats
                let encoding_file = EncodingFile::parse(&decoded_data).map_err(|e| {
                    InstallationError::InvalidConfiguration(format!(
                        "Failed to parse encoding manifest: {e}"
                    ))
                })?;

                if self.debug_mode {
                    eprintln!("EncodingFile::parse() completed successfully!");
                }

                let actual_size = decoded_data.len() as u64;
                let actual_entry_count = encoding_file.ckey_count();

                if self.debug_mode {
                    eprintln!("Encoding file parsed:");
                    eprintln!("  CKey entries: {actual_entry_count}");
                    eprintln!("  EKey entries: {}", encoding_file.ekey_count());
                }

                let manifest = EncodingManifest {
                    encoding_key: encoding_key_bytes,
                    size: actual_size,
                    entry_count: actual_entry_count,
                };

                (manifest, Some(encoding_file))
            }
            Err(e) => {
                // Failed to download, use info from BuildConfig
                // This is expected for large manifests that might timeout
                eprintln!(
                    "Note: Using BuildConfig info for encoding manifest (download failed: {e})"
                );
                let manifest = EncodingManifest {
                    encoding_key: encoding_key_bytes,
                    size: encoding_info.size.unwrap_or(15_000_000),
                    entry_count: 1_000_000, // Estimate for a typical encoding manifest
                };

                (manifest, None)
            }
        };

        // Process root manifest - need to look up encoding key via encoding manifest
        let root_manifest = if let Some(root_key_str) = root_content_key {
            // Skip root manifest download for utility products that don't have them
            if self.product_code == "agent" || self.product_code == "bna" {
                if self.debug_mode {
                    eprintln!("\n--- Skipping Root Manifest ---");
                    eprintln!(
                        "  {} is a utility product without root manifest",
                        self.product_code
                    );
                }
                RootManifest {
                    content_key: [0u8; 16],
                    encoding_key: [0u8; 16],
                    size: 0,
                    file_count: 0,
                    version: RootVersion::V1,
                }
            } else {
                if self.debug_mode {
                    eprintln!("\n--- Resolving Root Manifest ---");
                    eprintln!("Root content key: {}", &root_key_str);
                }

                let manifest = self
                    .resolve_root_manifest(
                        &encoding_manifest,
                        encoding_file_opt.as_ref(),
                        root_key_str,
                        &endpoint,
                        &configs.cdn_hosts,
                    )
                    .await?;

                if self.debug_mode {
                    eprintln!("Root manifest resolved successfully");
                }
                manifest
            }
        } else {
            // No root manifest for this product (agent, bna)
            if self.debug_mode {
                eprintln!("\n--- No Root Manifest ---");
                eprintln!(
                    "Creating placeholder root manifest for {}",
                    self.product_code
                );
            }
            RootManifest {
                content_key: [0u8; 16],
                encoding_key: [0u8; 16],
                size: 0,
                file_count: 0,
                version: RootVersion::V1,
            }
        };

        Ok(ManifestSet {
            encoding: encoding_manifest.clone(),
            root: root_manifest,
            install: self
                .resolve_install_manifest(
                    &encoding_manifest,
                    encoding_file_opt.as_ref(),
                    &install_info.content_key,
                    configs,
                    &endpoint,
                    &configs.cdn_hosts,
                )
                .await?,
            download: build_config
                .download()
                .first()
                .map(|dl_info| DownloadManifest {
                    content_key: hex::decode(&dl_info.content_key)
                        .ok()
                        .and_then(|k| k[..16].try_into().ok())
                        .unwrap_or([0u8; 16]),
                    encoding_key: [0u8; 16],
                    size: dl_info.size.unwrap_or(1_000_000),
                }),
        })
    }

    /// Download `BuildConfig` with fallback to community mirrors
    async fn download_build_config_with_fallback(
        &self,
        endpoint: &CdnEndpoint,
        hash: &str,
        cdn_hosts: &[String],
    ) -> Result<BuildConfig> {
        // Community CDN mirrors
        const COMMUNITY_MIRRORS: &[&str] = &[
            "cdn.arctium.tools",
            "casc.wago.tools",
            "tact.mirror.reliquaryhq.com",
        ];

        // Try primary CDN
        if self.debug_mode {
            eprintln!("  Trying primary CDN: {}", endpoint.host);
        }
        if let Ok(config) = self.download_build_config(endpoint, hash).await {
            if self.debug_mode {
                eprintln!("  ✓ Downloaded from primary CDN");
            }
            return Ok(config);
        }

        // Try alternate official CDNs
        for host in cdn_hosts {
            let mut alt_endpoint = endpoint.clone();
            alt_endpoint.host.clone_from(host);
            if let Ok(config) = self.download_build_config(&alt_endpoint, hash).await {
                eprintln!("Downloaded BuildConfig from alternate CDN: {host}");
                return Ok(config);
            }
        }

        // Try community mirrors (only for WoW products)
        // Community mirrors only host WoW content, not agent/bna
        if self.product_code.starts_with("wow") {
            for mirror in COMMUNITY_MIRRORS {
                let mut mirror_endpoint = endpoint.clone();
                mirror_endpoint.host = (*mirror).to_string();
                if let Ok(config) = self.download_build_config(&mirror_endpoint, hash).await {
                    eprintln!("Downloaded BuildConfig from community mirror: {mirror}");
                    return Ok(config);
                }
            }
        }

        Err(InstallationError::NetworkError(format!(
            "Failed to download BuildConfig {hash} from all sources"
        )))
    }

    /// Resolve root manifest by looking up encoding key and downloading from CDN
    async fn resolve_root_manifest(
        &self,
        _encoding_manifest: &EncodingManifest,
        encoding_file: Option<&EncodingFile>,
        root_content_key: &str,
        endpoint: &CdnEndpoint,
        cdn_hosts: &[String],
    ) -> Result<RootManifest> {
        let root_content_key_parsed = ContentKey::from_hex(root_content_key).map_err(|e| {
            InstallationError::InvalidConfiguration(format!("Invalid root content key: {e}"))
        })?;
        let root_key_bytes = *root_content_key_parsed.as_bytes();

        // If we have the parsed encoding file, look up the encoding key for the root manifest
        if let Some(encoding_file) = encoding_file {
            if self.debug_mode {
                eprintln!("  Looking up encoding key in parsed encoding file...");
                eprintln!("  Calling encoding_file.find_encoding()...");
            }
            // Look up the encoding key for this content key
            if let Some(encoding_key) = encoding_file.find_encoding(&root_content_key_parsed) {
                if self.debug_mode {
                    eprintln!(
                        "  Found encoding key: {}",
                        hex::encode(encoding_key.as_bytes())
                    );
                }
                // Download and parse the actual root manifest
                match self
                    .download_manifest_with_fallback(
                        endpoint,
                        &encoding_key.to_hex(),
                        ContentType::Data,
                        cdn_hosts,
                    )
                    .await
                {
                    Ok(decoded_data) => {
                        if self.debug_mode {
                            eprintln!("  Downloaded root manifest, parsing...");
                        }
                        // Parse the root manifest to get actual metadata
                        match cascette_formats::root::RootFile::parse(&decoded_data) {
                            Ok(root_file) => {
                                if self.debug_mode {
                                    eprintln!("  ✓ Root manifest parsed successfully");
                                    eprintln!("    Version: {:?}", root_file.version);
                                    eprintln!("    Total files: {}", root_file.total_files());
                                }
                                return Ok(RootManifest {
                                    content_key: root_key_bytes,
                                    encoding_key: *encoding_key.as_bytes(),
                                    size: decoded_data.len() as u64,
                                    version: root_file.version.into(),
                                    file_count: root_file.total_files() as usize,
                                });
                            }
                            Err(e) => {
                                eprintln!(
                                    "Failed to parse root manifest, using metadata estimate: {e}"
                                );
                                // Fall through to placeholder
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "Failed to download root manifest ({e}), using placeholder for utility product"
                        );
                        // For utility products like agent/bna, root manifest might not exist
                        if self.product_code == "agent" || self.product_code == "bna" {
                            eprintln!(
                                "Note: {} is a utility product, root manifest may not exist",
                                self.product_code
                            );
                        }
                        // Fall through to placeholder
                    }
                }
            } else {
                eprintln!("Root content key not found in encoding manifest, using placeholder");
            }
        }

        // Fallback to placeholder if encoding lookup or download failed
        Ok(RootManifest {
            content_key: root_key_bytes,
            encoding_key: [0u8; 16], // Will be resolved via encoding manifest parsing
            size: 8_300_000,         // Will be determined from actual download
            version: RootVersion::V4,
            file_count: 28456, // Will be determined when actually parsing
        })
    }

    /// Resolve install manifest by looking up encoding key and downloading from CDN
    async fn resolve_install_manifest(
        &self,
        _encoding_manifest: &EncodingManifest,
        encoding_file: Option<&EncodingFile>,
        install_content_key: &str,
        configs: &ConfigurationSet,
        endpoint: &CdnEndpoint,
        cdn_hosts: &[String],
    ) -> Result<InstallManifest> {
        let install_content_key_parsed =
            ContentKey::from_hex(install_content_key).map_err(|e| {
                InstallationError::InvalidConfiguration(format!("Invalid install content key: {e}"))
            })?;
        let install_key_bytes = *install_content_key_parsed.as_bytes();

        // If we have the parsed encoding file, look up the encoding key for the install manifest
        if let Some(encoding_file) = encoding_file {
            // Look up the encoding key for this content key
            if let Some(encoding_key) = encoding_file.find_encoding(&install_content_key_parsed) {
                // Download and parse the actual install manifest
                match self
                    .download_manifest_with_fallback(
                        endpoint,
                        &encoding_key.to_hex(),
                        ContentType::Data,
                        cdn_hosts,
                    )
                    .await
                {
                    Ok(decoded_data) => {
                        // Parse the install manifest to get actual metadata
                        match FormatsInstallManifest::parse(&decoded_data) {
                            Ok(install_manifest) => {
                                // Filter entries based on target tags to calculate actual install size
                                let filtered_entries =
                                    self.filter_install_entries(&install_manifest, &configs.tags);
                                let total_install_size: u64 = filtered_entries
                                    .iter()
                                    .map(|entry| u64::from(entry.file_size))
                                    .sum();

                                return Ok(InstallManifest {
                                    content_key: install_key_bytes,
                                    encoding_key: *encoding_key.as_bytes(),
                                    size: decoded_data.len() as u64,
                                    file_count: filtered_entries.len(),
                                    total_install_size,
                                    tags: configs.tags.clone(),
                                });
                            }
                            Err(e) => {
                                eprintln!(
                                    "Failed to parse install manifest, using metadata estimate: {e}"
                                );
                                // Fall through to placeholder
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "Failed to download install manifest, using metadata estimate: {e}"
                        );
                        // Fall through to placeholder
                    }
                }
            } else {
                eprintln!("Install content key not found in encoding manifest, using placeholder");
            }
        }

        // Fallback to placeholder if encoding lookup or download failed
        Ok(InstallManifest {
            content_key: install_key_bytes,
            encoding_key: [0u8; 16], // Will be resolved via encoding manifest parsing
            size: 2_100_000,         // Will be determined from actual download
            file_count: 28456,       // Will be determined when actually parsing
            total_install_size: 18_700_000_000, // Will be calculated from actual manifest
            tags: configs.tags.clone(),
        })
    }

    /// Filter install manifest entries based on platform/language tags
    /// Uses a multi-layer approach to handle corrupted manifests (like build 31650)
    #[allow(dead_code)] // Future manifest filtering
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
            // This is definitive and overrides tag data
            let path_lower = entry.path.to_lowercase();
            let is_definitely_macos = entry.path.contains(".app\\")
                || entry.path.contains(".app/")
                || entry.path.contains(".framework\\")
                || entry.path.contains(".framework/")
                || path_lower.ends_with(".dylib")
                || path_lower.ends_with(".icns");

            let is_definitely_windows = path_lower.ends_with(".exe")
                || path_lower.ends_with(".dll")
                || path_lower.ends_with(".mfil") // WoW manifest file
                || path_lower.ends_with(".bat");

            // Layer 2: Tag-based detection
            let has_windows_tag = file_tags.contains(&"Windows".to_string());
            let has_osx_tag = file_tags.contains(&"OSX".to_string());

            // Layer 3: Decision logic with path override
            let mut should_include = false;

            if is_targeting_windows {
                if is_definitely_macos {
                    // Path clearly indicates macOS - EXCLUDE regardless of tags
                    should_include = false;
                } else if is_definitely_windows {
                    // Path clearly indicates Windows - INCLUDE regardless of tags
                    should_include = true;
                } else {
                    // Path is ambiguous, use tag logic with OR semantics
                    // Include if: has Windows tag OR (doesn't have OSX tag)
                    if has_windows_tag {
                        should_include = true;
                    } else if !has_osx_tag {
                        // No clear platform tag, likely a shared file
                        should_include = true;
                    } else {
                        // Has OSX tag but not Windows tag
                        should_include = false;
                    }
                }
            } else if is_targeting_macos {
                if is_definitely_windows {
                    // Path clearly indicates Windows - EXCLUDE regardless of tags
                    should_include = false;
                } else if is_definitely_macos {
                    // Path clearly indicates macOS - INCLUDE regardless of tags
                    should_include = true;
                } else {
                    // Path is ambiguous, use tag logic with OR semantics
                    // Include if: has OSX tag OR (doesn't have Windows tag)
                    if has_osx_tag {
                        should_include = true;
                    } else if !has_windows_tag {
                        // No clear platform tag, likely a shared file
                        should_include = true;
                    } else {
                        // Has Windows tag but not OSX tag
                        should_include = false;
                    }
                }
            }

            // Now check non-platform tags (locale, architecture) with AND logic
            if should_include {
                for target_tag in target_tags {
                    // Skip platform tags - already handled above
                    if target_tag == "Windows" || target_tag == "OSX" {
                        continue;
                    }

                    // For non-platform tags, require exact match
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

    /// Download and parse `BuildConfig` from CDN
    async fn download_build_config(
        &self,
        endpoint: &CdnEndpoint,
        hash: &str,
    ) -> Result<BuildConfig> {
        // Convert hex string to bytes
        let key = hex::decode(hash).map_err(|e| {
            InstallationError::InvalidConfiguration(format!("Invalid BuildConfig hash: {e}"))
        })?;

        // Download the config file
        let data = self
            .cdn_client
            .download(endpoint, ContentType::Config, &key)
            .await
            .map_err(|e| {
                InstallationError::NetworkError(format!("Failed to download BuildConfig: {e}"))
            })?;

        // Parse the config
        BuildConfig::parse(&data[..]).map_err(|e| {
            InstallationError::InvalidConfiguration(format!("Failed to parse BuildConfig: {e}"))
        })
    }

    /// Download and parse `CDNConfig` from CDN
    async fn download_cdn_config(&self, endpoint: &CdnEndpoint, hash: &str) -> Result<CdnConfig> {
        // Convert hex string to bytes
        let key = hex::decode(hash).map_err(|e| {
            InstallationError::InvalidConfiguration(format!("Invalid CDNConfig hash: {e}"))
        })?;

        // Download the config file
        let data = self
            .cdn_client
            .download(endpoint, ContentType::Config, &key)
            .await
            .map_err(|e| {
                InstallationError::NetworkError(format!("Failed to download CDNConfig: {e}"))
            })?;

        // Parse the config
        CdnConfig::parse(&data[..]).map_err(|e| {
            InstallationError::InvalidConfiguration(format!("Failed to parse CDNConfig: {e}"))
        })
    }

    /// Download `CDNConfig` with fallback to community mirrors
    async fn download_cdn_config_with_fallback(
        &self,
        endpoint: &CdnEndpoint,
        hash: &str,
        cdn_hosts: &[String],
    ) -> Result<CdnConfig> {
        // Community CDN mirrors
        const COMMUNITY_MIRRORS: &[&str] = &[
            "cdn.arctium.tools",
            "casc.wago.tools",
            "tact.mirror.reliquaryhq.com",
        ];

        // Try primary CDN
        if self.debug_mode {
            eprintln!("  Trying primary CDN: {}", endpoint.host);
        }
        if let Ok(config) = self.download_cdn_config(endpoint, hash).await {
            if self.debug_mode {
                eprintln!("  ✓ Downloaded from primary CDN");
            }
            return Ok(config);
        }

        // Try alternate official CDNs
        for host in cdn_hosts {
            let mut alt_endpoint = endpoint.clone();
            alt_endpoint.host.clone_from(host);
            if let Ok(config) = self.download_cdn_config(&alt_endpoint, hash).await {
                eprintln!("Downloaded CDNConfig from alternate CDN: {host}");
                return Ok(config);
            }
        }

        // Try community mirrors (only for WoW products)
        // Community mirrors only host WoW content, not agent/bna
        if self.product_code.starts_with("wow") {
            for mirror in COMMUNITY_MIRRORS {
                let mut mirror_endpoint = endpoint.clone();
                mirror_endpoint.host = (*mirror).to_string();
                if let Ok(config) = self.download_cdn_config(&mirror_endpoint, hash).await {
                    eprintln!("Downloaded CDNConfig from community mirror: {mirror}");
                    return Ok(config);
                }
            }
        }

        Err(InstallationError::NetworkError(format!(
            "Failed to download CDNConfig {hash} from all sources"
        )))
    }

    /// Download any manifest with BLTE decoding, trying multiple CDN hosts
    async fn download_manifest_with_fallback(
        &self,
        endpoint: &CdnEndpoint,
        key: &str,
        content_type: ContentType,
        cdn_hosts: &[String],
    ) -> Result<Vec<u8>> {
        // Community CDN mirrors that maintain historic data (WoW only)
        const COMMUNITY_MIRRORS: &[&str] = &[
            "cdn.arctium.tools",
            "casc.wago.tools",
            "tact.mirror.reliquaryhq.com",
        ];

        if self.debug_mode {
            eprintln!("\n--- Download Manifest with Fallback ---");
            eprintln!("  Key: {key}");
            eprintln!("  Content type: {content_type:?}");
            eprintln!("  Primary endpoint: {endpoint:?}");
        }

        // For WoW products, try official CDN hosts first, then community mirrors as fallback
        // For other products (agent, bna), only use official CDN hosts
        let all_hosts: Vec<String> = if self.product_code.starts_with("wow") {
            cdn_hosts
                .iter()
                .cloned()
                .chain(COMMUNITY_MIRRORS.iter().map(|&s| s.to_string()))
                .collect()
        } else {
            cdn_hosts.to_vec()
        };

        if self.debug_mode {
            eprintln!("  Available hosts: {all_hosts:?}");
        }

        let mut last_error = None;
        let mut last_url = None;

        for host in all_hosts {
            let host_endpoint = CdnEndpoint {
                host: host.clone(),
                path: endpoint.path.clone(),
                product_path: endpoint.product_path.clone(),
                scheme: None,
            };

            let key_bytes = hex::decode(key).map_err(|e| {
                InstallationError::InvalidConfiguration(format!("Invalid hex key {key}: {e}"))
            })?;

            // Build the URL for error reporting
            let base_path = if let Some(ref product_path) = host_endpoint.product_path {
                format!("{}/{}", host_endpoint.path, product_path)
            } else {
                host_endpoint.path.clone()
            };
            let url = format!(
                "http://{}/{}/{}/{}",
                host_endpoint.host, base_path, content_type, key
            );

            if self.debug_mode {
                eprintln!("\n  Trying host: {host}");
                eprintln!("  Full URL: {url}");
                eprintln!("  Downloading from CDN...");
            }

            match self
                .cdn_client
                .download(&host_endpoint, content_type, &key_bytes)
                .await
            {
                Ok(data) => {
                    if self.debug_mode {
                        eprintln!("  ✓ Download successful, received {} bytes", data.len());
                        eprintln!("  Parsing BLTE container...");
                    }

                    // BLTE decode the content
                    let blte_file = BlteFile::parse(&data).map_err(|e| {
                        if self.debug_mode {
                            eprintln!("  ✗ BLTE parse failed: {e}");
                        }
                        InstallationError::InvalidConfiguration(format!(
                            "Failed to parse BLTE for {key}: {e}"
                        ))
                    })?;

                    if self.debug_mode {
                        eprintln!("  ✓ BLTE parsed successfully");
                        eprintln!("  Decompressing BLTE data...");
                    }

                    let decoded = blte_file.decompress().map_err(|e| {
                        if self.debug_mode {
                            eprintln!("  ✗ BLTE decompression failed: {e}");
                        }
                        InstallationError::InvalidConfiguration(format!(
                            "Failed to decompress BLTE for {key}: {e}"
                        ))
                    })?;

                    if self.debug_mode {
                        eprintln!(
                            "  ✓ BLTE decompressed successfully, {} bytes",
                            decoded.len()
                        );
                    }

                    return Ok(decoded);
                }
                Err(e) => {
                    if self.debug_mode {
                        eprintln!("  ✗ Download failed: {e}");
                    }
                    // Store the error but don't print it yet - only print if all mirrors fail
                    last_error = Some(e);
                    last_url = Some(url);
                }
            }
        }

        // Only print error message when all mirrors have failed
        if let (Some(last_err), Some(url)) = (last_error, last_url) {
            if self.debug_mode {
                eprintln!(
                    "\nFailed to download {content_type:?} from all mirrors. Last URL attempted: {url}\nError: {last_err}"
                );
            }
        }

        Err(InstallationError::NetworkError(format!(
            "Failed to download {content_type:?} {key} from all sources"
        )))
    }

    /// Download encoding manifest with fallback to community mirrors
    async fn download_encoding_manifest_with_fallback(
        &self,
        endpoint: &CdnEndpoint,
        encoding_key: &[u8],
        cdn_hosts: &[String],
    ) -> Result<Vec<u8>> {
        // Community CDN mirrors that maintain historic data (WoW only)
        const COMMUNITY_MIRRORS: &[&str] = &[
            "cdn.arctium.tools",
            "casc.wago.tools",
            "tact.mirror.reliquaryhq.com",
        ];

        if self.debug_mode {
            eprintln!("  Endpoint: {endpoint:?}");
        }

        // Try official CDN first
        if self.debug_mode {
            eprintln!("  Trying primary CDN...");
        }
        if let Ok(data) = self
            .download_encoding_manifest(endpoint, encoding_key)
            .await
        {
            if self.debug_mode {
                eprintln!("  ✓ Downloaded from primary CDN");
            }
            return Ok(data);
        }

        // Try other official CDN hosts
        for host in cdn_hosts {
            let mut alt_endpoint = endpoint.clone();
            alt_endpoint.host.clone_from(host);
            if let Ok(data) = self
                .download_encoding_manifest(&alt_endpoint, encoding_key)
                .await
            {
                eprintln!("Downloaded encoding manifest from alternate CDN: {host}");
                return Ok(data);
            }
        }

        // Try community mirrors for historic data (WoW only)
        if self.product_code.starts_with("wow") {
            for mirror in COMMUNITY_MIRRORS {
                let mut mirror_endpoint = endpoint.clone();
                mirror_endpoint.host = (*mirror).to_string();
                if let Ok(data) = self
                    .download_encoding_manifest(&mirror_endpoint, encoding_key)
                    .await
                {
                    eprintln!("Downloaded encoding manifest from community mirror: {mirror}");
                    return Ok(data);
                }
            }
        } // Close the if statement for WoW products

        Err(InstallationError::NetworkError(
            "Failed to download encoding manifest from all available sources".to_string(),
        ))
    }

    /// Download encoding manifest from CDN with caching
    async fn download_encoding_manifest(
        &self,
        endpoint: &CdnEndpoint,
        encoding_key: &[u8],
    ) -> Result<Vec<u8>> {
        use binrw::BinRead;
        use std::io::Cursor;

        if self.debug_mode {
            eprintln!("\n--- Direct Download Encoding Manifest ---");
            eprintln!("  Endpoint: {}@{}", endpoint.host, endpoint.path);
            eprintln!("  Encoding key: {}", hex::encode(encoding_key));
        }

        // The encoding manifest is downloaded from the data endpoint using its content key
        // It will be automatically cached by the CDN client
        let blte_data = self
            .cdn_client
            .download(endpoint, ContentType::Data, encoding_key)
            .await
            .map_err(|e| {
                if self.debug_mode {
                    eprintln!("  ✗ Download failed: {e}");
                }
                InstallationError::NetworkError(format!(
                    "Failed to download encoding manifest: {e}"
                ))
            })?;

        if self.debug_mode {
            eprintln!("  ✓ Download successful, {} bytes", blte_data.len());
            eprintln!("  Parsing BLTE with BinRead...");
        }

        // Parse and decompress the BLTE data

        let mut cursor = Cursor::new(&blte_data);
        let blte_file = BlteFile::read_le(&mut cursor).map_err(|e| {
            if self.debug_mode {
                eprintln!("  ✗ BLTE parse failed: {e}");
            }
            InstallationError::InvalidConfiguration(format!("Failed to parse BLTE data: {e}"))
        })?;

        if self.debug_mode {
            eprintln!("  ✓ BLTE parsed successfully");
            eprintln!("  Decompressing BLTE chunks...");
        }

        // Decompress all chunks to get the actual encoding manifest
        let decoded_data = blte_file.decompress().map_err(|e| {
            if self.debug_mode {
                eprintln!("  ✗ BLTE decompression failed: {e}");
            }
            InstallationError::InvalidConfiguration(format!("Failed to decompress BLTE data: {e}"))
        })?;

        if self.debug_mode {
            eprintln!(
                "  ✓ BLTE decompressed successfully, {} bytes",
                decoded_data.len()
            );
        }

        Ok(decoded_data)
    }

    /// Resolve archive information from CDN config
    pub async fn resolve_archives(&self, configs: &ConfigurationSet) -> Result<ArchiveSet> {
        // Check if this is a historic build - if so, we can't download archive info
        if let Some(build_id) = self.build_id {
            if let Some(ref build_manager) = self.build_manager {
                if build_manager.build_exists(&self.product_code, build_id) {
                    // Historic build - archive info not available on CDN
                    return Ok(ArchiveSet {
                        archives: vec![],                  // No archive info available
                        indices: vec![],                   // No index info available
                        total_archive_size: 4_500_000_000, // Classic is ~4.5GB
                    });
                }
            }
        }

        // Create CDN endpoint
        let endpoint = CdnEndpoint {
            host: configs
                .cdn_hosts
                .first()
                .ok_or_else(|| {
                    InstallationError::NetworkError("No CDN hosts available".to_string())
                })?
                .clone(),
            path: configs.cdn_path.clone(),
            product_path: None,
            scheme: None,
        };

        // Download and parse CDNConfig with fallback
        let cdn_config = self
            .download_cdn_config_with_fallback(&endpoint, &configs.cdn_config, &configs.cdn_hosts)
            .await?;

        // Extract archive information
        let archive_infos = cdn_config.archives();

        // Calculate total size (this is approximate as we only have index sizes)
        let total_size: u64 = archive_infos
            .iter()
            .filter_map(|a| a.index_size)
            .sum::<u64>()
            * 1000; // Rough estimate: archives are ~1000x index size

        // Convert to our model format
        let archives: Vec<ArchiveInfo> = archive_infos
            .iter()
            .map(|a| ArchiveInfo {
                hash: a.content_key.clone(),
                size: a.index_size.unwrap_or(0) * 1000, // Estimate archive size from index
            })
            .collect();

        let indices: Vec<ArchiveIndexInfo> = archive_infos
            .iter()
            .map(|a| ArchiveIndexInfo {
                archive_hash: a.content_key.clone(),
                index_size: a.index_size.unwrap_or(0),
                entry_count: 0, // Will be determined when parsing
            })
            .collect();

        Ok(ArchiveSet {
            archives,
            indices,
            total_archive_size: if total_size > 0 {
                total_size
            } else {
                15_200_000_000
            }, // Use default if no size info
        })
    }

    /// Get CDN information from NGDP
    async fn get_cdn_info(&self) -> Result<CdnInfo> {
        let endpoint = format!("v1/products/{}/cdns", self.product_code);
        let cdns =
            self.client.query(&endpoint).await.map_err(|e| {
                InstallationError::NetworkError(format!("Failed to query CDNs: {e}"))
            })?;

        // For now, use the first CDN configuration (typically 'us')
        // In a full implementation, we'd match based on region preference
        let cdn_row = cdns.rows().first().ok_or_else(|| {
            InstallationError::NetworkError("No CDN configurations found".to_string())
        })?;

        // Extract the fields we need - using the actual field names from NGDP
        let hosts = cdn_row
            .get_by_name("Hosts", cdns.schema())
            .and_then(|v| v.as_string())
            .ok_or_else(|| InstallationError::NetworkError("Missing Hosts field".to_string()))?;

        let path = cdn_row
            .get_by_name("Path", cdns.schema())
            .and_then(|v| v.as_string())
            .ok_or_else(|| InstallationError::NetworkError("Missing Path field".to_string()))?;

        // Parse all CDN hosts from the space-separated list
        let hosts: Vec<String> = hosts.split_whitespace().map(String::from).collect();

        Ok(CdnInfo {
            hosts,
            path: path.to_string(),
        })
    }

    /// Detect current platform and generate appropriate NGDP tags
    #[must_use]
    pub fn detect_platform_tags() -> Vec<String> {
        let mut tags = Vec::new();

        // Platform detection
        #[cfg(target_os = "windows")]
        {
            tags.push("Windows".to_string());
        }
        #[cfg(target_os = "macos")]
        {
            tags.push("OSX".to_string()); // NGDP uses "OSX" not "macOS"
        }
        #[cfg(target_os = "linux")]
        {
            // Linux users need Windows binaries to run games through Wine/Proton
            // Since Blizzard doesn't ship native Linux builds, we use Windows tags
            tags.push("Windows".to_string());
        }

        // Architecture detection
        #[cfg(target_arch = "x86_64")]
        {
            tags.push("x86_64".to_string());
        }
        #[cfg(target_arch = "aarch64")]
        {
            tags.push("aarch64".to_string()); // ARM64 (Apple Silicon)
        }

        // Default to English US locale
        tags.push("enUS".to_string());

        // NOTE: We do NOT add "speech" and "text" tags here.
        // Those are content-specific tags that many core files (like executables) don't have.
        // Including them would filter out essential files like WowClassic.exe.

        tags
    }
}

/// CDN information
struct CdnInfo {
    hosts: Vec<String>,
    path: String,
}
