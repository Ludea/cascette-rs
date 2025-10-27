//! Build metadata structures for NGDP/CASC builds
//!
//! This module provides all data structures for storing build metadata
//! in a unified format that supports both live NGDP queries and historic
//! data imports from wago.tools.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Complete build metadata for an NGDP/CASC build
///
/// Contains all information needed to identify, download, and install a specific build,
/// including configuration hashes, CDN endpoints, and regional data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildMetadata {
    /// Metadata about this record
    pub meta: MetadataInfo,

    /// Build identification
    pub build: BuildInfo,

    /// Configuration hashes
    pub configs: ConfigInfo,

    /// CDN distribution information
    pub cdn: CdnInfo,

    /// Region-specific information
    pub regions: HashMap<String, RegionInfo>,

    /// Product-specific information
    pub product: ProductInfo,

    /// Optional patch information
    pub patch: Option<PatchInfo>,

    /// Optional version catalog information (from wago.tools)
    pub catalog: Option<CatalogInfo>,
}

/// Metadata information about the build record itself
///
/// Tracks when and how this build metadata was captured or imported.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataInfo {
    /// When this metadata was captured/imported
    pub captured_at: DateTime<Utc>,

    /// Source of the metadata
    pub source: DataSource,

    /// Schema version for forward compatibility
    pub schema_version: u32,

    /// Last time this metadata was updated
    pub updated_at: DateTime<Utc>,

    /// Unique identifier for this build
    pub build_id: String,
}

/// Source of the build metadata
///
/// Indicates where the build metadata came from (live query, import, or manual entry).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DataSource {
    /// Live NGDP query
    Live {
        /// Region queried (e.g., "us", "eu")
        region: String,
        /// Ribbit endpoint used
        endpoint: String,
        /// When the query was performed
        query_time: DateTime<Utc>,
    },
    /// Imported from wago.tools
    Wago {
        /// When the data was imported
        import_time: DateTime<Utc>,
        /// Wago.tools version ID if available
        wago_version_id: Option<String>,
    },
    /// Manually created or edited
    Manual {
        /// Who created this entry
        created_by: String,
        /// Reason for manual creation
        reason: String,
    },
}

/// Build identification information
///
/// Contains all identifying information for a specific NGDP build including
/// product code, version, and build number.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildInfo {
    /// Product code (e.g., "wow", "`wow_classic`")
    pub product_code: String,

    /// Version string (e.g., "1.15.7", "11.0.5")
    pub version: String,

    /// Build number (e.g., 61582, 57689)
    pub build_number: u32,

    /// Combined version-build string (e.g., "1.15.7.61582")
    pub version_build: String,

    /// Build name from build config (e.g., "WOW-55646patch1.15.3_ClassicRetail")
    pub build_name: Option<String>,

    /// Build UID from build config (e.g., "`wow_classic_era`")
    pub build_uid: Option<String>,

    /// Build product from build config (e.g., "`WoW`")
    pub build_product: Option<String>,

    /// Branch name if known (e.g., "retail", "classic", "ptr")
    pub branch: Option<String>,
}

/// Configuration hashes for a build
///
/// Contains hashes for all configuration files and executable configs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigInfo {
    /// Build configuration hash
    pub build_config: String,

    /// CDN configuration hash
    pub cdn_config: String,

    /// Product configuration hash (optional)
    pub product_config: Option<String>,

    /// Patch configuration hash (optional)
    pub patch_config: Option<String>,

    /// Executable configuration hashes (per architecture)
    pub exe_configs: HashMap<String, String>,
}

/// CDN distribution information
///
/// Contains CDN hosts, paths, and protocol information for downloading build files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdnInfo {
    /// CDN hosts for this build
    pub hosts: Vec<String>,

    /// CDN path prefix (e.g., "tpr/wow")
    pub path: String,

    /// Product path prefix for configs (e.g., "tpr/configs")
    pub product_path: Option<String>,

    /// Supported protocols
    pub protocols: Vec<CdnProtocol>,

    /// Archive group identifier
    pub archive_group: Option<String>,

    /// Number of archives
    pub archive_count: Option<usize>,
}

/// CDN protocol specification
///
/// Specifies protocol and port for CDN access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdnProtocol {
    /// Protocol name ("http" or "https")
    pub protocol: String,
    /// Port number (80 or 443)
    pub port: u16,
}

/// Region-specific information
///
/// Contains region availability and configuration overrides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionInfo {
    /// Region code (e.g., "us", "eu", "kr")
    pub region: String,

    /// Whether this build is available in this region
    pub available: bool,

    /// Region-specific CDN hosts if different
    pub cdn_hosts: Option<Vec<String>>,

    /// Version string for this region if different
    pub version_string: Option<String>,
}

/// Product information and requirements
///
/// Contains product metadata including name, type, and platform support.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductInfo {
    /// Human-readable product name
    pub display_name: String,

    /// Product family (e.g., "wow", "diablo", "starcraft")
    pub family: String,

    /// Product type (e.g., "game", "tool", "launcher")
    pub product_type: String,

    /// Supported platforms
    pub platforms: Vec<String>,

    /// Required subscription type if applicable
    pub subscription: Option<String>,
}

/// Patch information for incremental updates
///
/// Contains patch manifest and size information for differential updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchInfo {
    /// Previous build this patches from
    pub from_build: String,

    /// Patch manifest content key
    pub patch_manifest_key: String,

    /// Size of patch data
    pub patch_size: u64,

    /// Whether this is a partial patch
    pub is_partial: bool,
}

/// Catalog information from version system
///
/// Contains release metadata and flags from Ribbit/version catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogInfo {
    /// Sequence number from Ribbit
    pub sequence_number: Option<u64>,

    /// Flags from catalog (e.g., "installed", "critical")
    pub flags: Vec<String>,

    /// Version tags (e.g., `["ptr", "beta", "live"]`)
    pub tags: Vec<String>,

    /// Release date if known
    pub release_date: Option<DateTime<Utc>>,

    /// End of support date if known
    pub end_of_support: Option<DateTime<Utc>>,
}

/// Helper function to parse version string and extract components
#[allow(dead_code)]
pub fn parse_version_build(version_build: &str) -> anyhow::Result<(String, u32)> {
    // Parse "1.15.7.61582" into version "1.15.7" and build 61582
    let parts: Vec<&str> = version_build.split('.').collect();
    if parts.len() < 4 {
        return Err(anyhow::anyhow!("Invalid version format: {version_build}"));
    }

    let version = parts[0..parts.len() - 1].join(".");
    let build_number = parts[parts.len() - 1].parse::<u32>()?;

    Ok((version, build_number))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_fun_call,
    clippy::unreadable_literal,
    clippy::panic
)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::collections::HashMap;

    fn create_test_metadata() -> BuildMetadata {
        let now = Utc::now();

        let mut regions = HashMap::new();
        regions.insert(
            "us".to_string(),
            RegionInfo {
                region: "us".to_string(),
                available: true,
                cdn_hosts: None,
                version_string: Some("1.15.7.61582".to_string()),
            },
        );

        let mut exe_configs = HashMap::new();
        exe_configs.insert("win_x64".to_string(), "abc123def456".to_string());
        exe_configs.insert("win_arm64".to_string(), "def789abc012".to_string());

        BuildMetadata {
            meta: MetadataInfo {
                captured_at: now,
                source: DataSource::Live {
                    region: "us".to_string(),
                    endpoint: "us.version.battle.net".to_string(),
                    query_time: now,
                },
                schema_version: 1,
                updated_at: now,
                build_id: "wow_classic_era:1.15.7.61582:bbf06e7476382cfaa396cff0049d356b"
                    .to_string(),
            },
            build: BuildInfo {
                product_code: "wow_classic_era".to_string(),
                version: "1.15.7".to_string(),
                build_number: 61582,
                version_build: "1.15.7.61582".to_string(),
                build_name: Some("WOW-61582patch1.15.7_ClassicRetail".to_string()),
                build_uid: Some("wow_classic_era".to_string()),
                build_product: Some("WoW".to_string()),
                branch: Some("classic_era".to_string()),
            },
            configs: ConfigInfo {
                build_config: "ae66faee0ac786fdd7d8b4cf90a8d5b9".to_string(),
                cdn_config: "63eee50d456a6ddf3b630957c024dda0".to_string(),
                product_config: Some("53020d32e1a25648c8e1eafd5771935f".to_string()),
                patch_config: Some("474b9630df5b46df5d98ec27c5f78d07".to_string()),
                exe_configs,
            },
            cdn: CdnInfo {
                hosts: vec![
                    "cdn.arctium.tools".to_string(),
                    "level3.blizzard.com".to_string(),
                    "cdn.blizzard.com".to_string(),
                ],
                path: "tpr/wow".to_string(),
                product_path: Some("tpr/configs".to_string()),
                protocols: vec![
                    CdnProtocol {
                        protocol: "https".to_string(),
                        port: 443,
                    },
                    CdnProtocol {
                        protocol: "http".to_string(),
                        port: 80,
                    },
                ],
                archive_group: Some("58a3c9e02c964b0ec9dd6c085df99a77".to_string()),
                archive_count: Some(1247),
            },
            regions,
            product: ProductInfo {
                display_name: "World of Warcraft Classic Era".to_string(),
                family: "wow".to_string(),
                product_type: "game".to_string(),
                platforms: vec!["windows".to_string(), "macos".to_string()],
                subscription: Some("wow_classic".to_string()),
            },
            patch: None,
            catalog: Some(CatalogInfo {
                sequence_number: Some(2847),
                flags: vec!["installed".to_string()],
                tags: vec!["live".to_string(), "classic_era".to_string()],
                release_date: Some(Utc::now()),
                end_of_support: None,
            }),
        }
    }

    #[test]
    fn test_build_metadata_serialization() {
        let metadata = create_test_metadata();

        // Test serialization
        let json = serde_json::to_string_pretty(&metadata).expect("Failed to serialize");
        assert!(!json.is_empty());
        assert!(json.contains("wow_classic_era"));
        assert!(json.contains("61582"));

        // Test deserialization
        let deserialized: BuildMetadata =
            serde_json::from_str(&json).expect("Failed to deserialize");

        // Verify key fields match
        assert_eq!(deserialized.build.product_code, "wow_classic_era");
        assert_eq!(deserialized.build.build_number, 61582);
        assert_eq!(deserialized.build.version, "1.15.7");
        assert_eq!(
            deserialized.configs.build_config,
            "ae66faee0ac786fdd7d8b4cf90a8d5b9"
        );
    }

    #[test]
    fn test_data_source_serialization() {
        let live_source = DataSource::Live {
            region: "us".to_string(),
            endpoint: "us.version.battle.net".to_string(),
            query_time: Utc::now(),
        };

        let json = serde_json::to_string(&live_source).expect("Failed to serialize live source");
        assert!(json.contains("Live"));
        assert!(json.contains("us.version.battle.net"));

        let deserialized: DataSource =
            serde_json::from_str(&json).expect("Failed to deserialize live source");

        if let DataSource::Live {
            region, endpoint, ..
        } = deserialized
        {
            assert_eq!(region, "us");
            assert_eq!(endpoint, "us.version.battle.net");
        } else {
            panic!("Expected Live data source");
        }
    }

    #[test]
    fn test_wago_data_source() {
        let wago_source = DataSource::Wago {
            import_time: Utc::now(),
            wago_version_id: Some("12345".to_string()),
        };

        let json = serde_json::to_string(&wago_source).expect("Failed to serialize wago source");
        let deserialized: DataSource =
            serde_json::from_str(&json).expect("Failed to deserialize wago source");

        if let DataSource::Wago {
            wago_version_id, ..
        } = deserialized
        {
            assert_eq!(wago_version_id, Some("12345".to_string()));
        } else {
            panic!("Expected Wago data source");
        }
    }

    #[test]
    fn test_manual_data_source() {
        let manual_source = DataSource::Manual {
            created_by: "admin".to_string(),
            reason: "Testing".to_string(),
        };

        let json =
            serde_json::to_string(&manual_source).expect("Failed to serialize manual source");
        let deserialized: DataSource =
            serde_json::from_str(&json).expect("Failed to deserialize manual source");

        if let DataSource::Manual { created_by, reason } = deserialized {
            assert_eq!(created_by, "admin");
            assert_eq!(reason, "Testing");
        } else {
            panic!("Expected Manual data source");
        }
    }

    #[test]
    fn test_build_info_fields() {
        let build_info = BuildInfo {
            product_code: "wow_classic_era".to_string(),
            version: "1.15.7".to_string(),
            build_number: 61582,
            version_build: "1.15.7.61582".to_string(),
            build_name: Some("WOW-61582patch1.15.7_ClassicRetail".to_string()),
            build_uid: Some("wow_classic_era".to_string()),
            build_product: Some("WoW".to_string()),
            branch: Some("classic_era".to_string()),
        };

        assert_eq!(build_info.product_code, "wow_classic_era");
        assert_eq!(build_info.version, "1.15.7");
        assert_eq!(build_info.build_number, 61582);
        assert_eq!(build_info.version_build, "1.15.7.61582");

        // Test serialization preserves separate version and build_number fields
        let json = serde_json::to_string(&build_info).expect("Failed to serialize build info");
        assert!(json.contains("\"version\":\"1.15.7\""));
        assert!(json.contains("\"build_number\":61582"));
    }

    #[test]
    fn test_config_info_with_exe_configs() {
        let mut exe_configs = HashMap::new();
        exe_configs.insert("win_x64".to_string(), "abc123".to_string());
        exe_configs.insert("mac_x64".to_string(), "def456".to_string());

        let config_info = ConfigInfo {
            build_config: "build_hash".to_string(),
            cdn_config: "cdn_hash".to_string(),
            product_config: Some("product_hash".to_string()),
            patch_config: None,
            exe_configs,
        };

        assert_eq!(config_info.exe_configs.len(), 2);
        assert_eq!(
            config_info.exe_configs.get("win_x64"),
            Some(&"abc123".to_string())
        );
        assert_eq!(
            config_info.exe_configs.get("mac_x64"),
            Some(&"def456".to_string())
        );
    }

    #[test]
    fn test_cdn_info_with_protocols() {
        let protocols = vec![
            CdnProtocol {
                protocol: "https".to_string(),
                port: 443,
            },
            CdnProtocol {
                protocol: "http".to_string(),
                port: 80,
            },
        ];

        let cdn_info = CdnInfo {
            hosts: vec![
                "cdn1.example.com".to_string(),
                "cdn2.example.com".to_string(),
            ],
            path: "tpr/wow".to_string(),
            product_path: Some("tpr/configs".to_string()),
            protocols,
            archive_group: Some("archive_group_hash".to_string()),
            archive_count: Some(1500),
        };

        assert_eq!(cdn_info.protocols.len(), 2);
        assert_eq!(cdn_info.protocols[0].protocol, "https");
        assert_eq!(cdn_info.protocols[0].port, 443);
        assert_eq!(cdn_info.archive_count, Some(1500));
    }

    #[test]
    fn test_region_info() {
        let region_info = RegionInfo {
            region: "eu".to_string(),
            available: true,
            cdn_hosts: Some(vec!["eu-cdn.example.com".to_string()]),
            version_string: Some("1.15.7.61582".to_string()),
        };

        assert_eq!(region_info.region, "eu");
        assert!(region_info.available);
        assert_eq!(
            region_info.cdn_hosts.as_ref().expect("Test assertion")[0],
            "eu-cdn.example.com"
        );
    }

    #[test]
    fn test_patch_info() {
        let patch_info = PatchInfo {
            from_build: "60000".to_string(),
            patch_manifest_key: "patch_key_hash".to_string(),
            patch_size: 1024 * 1024 * 100, // 100MB
            is_partial: false,
        };

        assert_eq!(patch_info.from_build, "60000");
        assert_eq!(patch_info.patch_size, 104857600);
        assert!(!patch_info.is_partial);
    }

    #[test]
    fn test_catalog_info() {
        let catalog_info = CatalogInfo {
            sequence_number: Some(12345),
            flags: vec!["installed".to_string(), "critical".to_string()],
            tags: vec!["live".to_string(), "retail".to_string()],
            release_date: Some(Utc::now()),
            end_of_support: None,
        };

        assert_eq!(catalog_info.sequence_number, Some(12345));
        assert_eq!(catalog_info.flags.len(), 2);
        assert_eq!(catalog_info.tags.len(), 2);
        assert!(catalog_info.release_date.is_some());
    }

    #[test]
    fn test_parse_version_build_success() {
        let (version, build_number) =
            parse_version_build("1.15.7.61582").expect("Failed to parse version build");

        assert_eq!(version, "1.15.7");
        assert_eq!(build_number, 61582);
    }

    #[test]
    fn test_parse_version_build_different_formats() {
        // Test various version formats
        let test_cases = vec![
            ("11.0.5.57689", "11.0.5", 57689),
            ("10.2.7.54577", "10.2.7", 54577),
            ("5.4.8.18414", "5.4.8", 18414),
        ];

        for (input, expected_version, expected_build) in test_cases {
            let (version, build_number) =
                parse_version_build(input).expect(&format!("Failed to parse {input}"));

            assert_eq!(version, expected_version, "Version mismatch for {input}");
            assert_eq!(
                build_number, expected_build,
                "Build number mismatch for {input}"
            );
        }
    }

    #[test]
    fn test_parse_version_build_invalid_format() {
        let invalid_cases = vec![
            "1.15.7",     // Missing build number
            "1.15",       // Too few parts
            "invalid",    // Not version format
            "1.15.7.abc", // Non-numeric build
        ];

        for invalid_input in invalid_cases {
            let result = parse_version_build(invalid_input);
            assert!(result.is_err(), "Expected error for input: {invalid_input}");
        }
    }

    #[test]
    fn test_complete_json_round_trip() {
        let metadata = create_test_metadata();

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&metadata).expect("Failed to serialize metadata");

        // Deserialize from JSON
        let deserialized: BuildMetadata =
            serde_json::from_str(&json).expect("Failed to deserialize metadata");

        // Verify all major fields
        assert_eq!(deserialized.meta.build_id, metadata.meta.build_id);
        assert_eq!(deserialized.build.product_code, metadata.build.product_code);
        assert_eq!(deserialized.build.build_number, metadata.build.build_number);
        assert_eq!(deserialized.build.version, metadata.build.version);
        assert_eq!(
            deserialized.configs.build_config,
            metadata.configs.build_config
        );
        assert_eq!(deserialized.cdn.hosts, metadata.cdn.hosts);
        assert_eq!(
            deserialized.product.display_name,
            metadata.product.display_name
        );

        // Verify regions
        assert_eq!(deserialized.regions.len(), 1);
        assert!(deserialized.regions.contains_key("us"));

        // Verify catalog is preserved
        assert!(deserialized.catalog.is_some());
        let catalog = deserialized.catalog.expect("Test assertion");
        assert_eq!(catalog.sequence_number, Some(2847));
        assert_eq!(catalog.flags, vec!["installed"]);
    }

    #[test]
    fn test_optional_fields() {
        // Test metadata with minimal required fields and many optional fields as None
        let now = Utc::now();

        let minimal_metadata = BuildMetadata {
            meta: MetadataInfo {
                captured_at: now,
                source: DataSource::Manual {
                    created_by: "test".to_string(),
                    reason: "testing".to_string(),
                },
                schema_version: 1,
                updated_at: now,
                build_id: "test:12345:hash".to_string(),
            },
            build: BuildInfo {
                product_code: "test_product".to_string(),
                version: "1.0.0".to_string(),
                build_number: 12345,
                version_build: "1.0.0.12345".to_string(),
                build_name: None,
                build_uid: None,
                build_product: None,
                branch: None,
            },
            configs: ConfigInfo {
                build_config: "build_hash".to_string(),
                cdn_config: "cdn_hash".to_string(),
                product_config: None,
                patch_config: None,
                exe_configs: HashMap::new(),
            },
            cdn: CdnInfo {
                hosts: vec!["cdn.example.com".to_string()],
                path: "test/path".to_string(),
                product_path: None,
                protocols: vec![],
                archive_group: None,
                archive_count: None,
            },
            regions: HashMap::new(),
            product: ProductInfo {
                display_name: "Test Product".to_string(),
                family: "test".to_string(),
                product_type: "game".to_string(),
                platforms: vec!["test".to_string()],
                subscription: None,
            },
            patch: None,
            catalog: None,
        };

        // Verify serialization works with optional fields as None
        let json =
            serde_json::to_string(&minimal_metadata).expect("Failed to serialize minimal metadata");

        let deserialized: BuildMetadata =
            serde_json::from_str(&json).expect("Failed to deserialize minimal metadata");

        // Verify None fields remain None
        assert!(deserialized.build.build_name.is_none());
        assert!(deserialized.configs.product_config.is_none());
        assert!(deserialized.cdn.archive_group.is_none());
        assert!(deserialized.patch.is_none());
        assert!(deserialized.catalog.is_none());
    }
}
