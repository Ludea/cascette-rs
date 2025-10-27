//! Build manager for storing and retrieving build metadata locally

use crate::builds::metadata::BuildMetadata;
use anyhow::{Result, anyhow};
use std::fs;
use std::path::{Path, PathBuf};

/// Manages build metadata storage and retrieval
///
/// The `BuildManager` stores build metadata locally in JSON format,
/// organized by product code and build number.
#[allow(dead_code)]
pub struct BuildManager {
    data_dir: PathBuf,
}

#[allow(dead_code)]
impl BuildManager {
    /// Create a new build manager with the specified data directory
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            data_dir: data_dir.as_ref().to_path_buf(),
        }
    }

    /// Save build metadata to disk
    pub fn save_build(&self, metadata: &BuildMetadata) -> Result<()> {
        let filename = format!("{}.json", metadata.build.build_number);

        let dir = self
            .data_dir
            .join("builds")
            .join(&metadata.build.product_code);

        fs::create_dir_all(&dir)?;

        let path = dir.join(filename);
        let json = serde_json::to_string_pretty(metadata)?;
        fs::write(path, json)?;

        Ok(())
    }

    /// Load build metadata from disk by build number
    pub fn load_build(&self, product_code: &str, build_number: u32) -> Result<BuildMetadata> {
        let path = self
            .data_dir
            .join("builds")
            .join(product_code)
            .join(format!("{build_number}.json"));

        if !path.exists() {
            return Err(anyhow!("Build not found: {product_code}:{build_number}"));
        }

        let data = fs::read_to_string(path)?;
        let metadata: BuildMetadata = serde_json::from_str(&data)?;
        Ok(metadata)
    }

    /// Load build metadata by full build ID (includes config hash)
    pub fn load_build_by_id(&self, build_id: &str) -> Result<BuildMetadata> {
        // Parse build_id format: product:build_number:config_hash
        let parts: Vec<&str> = build_id.split(':').collect();
        if parts.len() != 3 {
            return Err(anyhow!("Invalid build ID format: {build_id}"));
        }

        let build_number: u32 = parts[1].parse()?;
        let metadata = self.load_build(parts[0], build_number)?;

        // Verify config hash matches
        if metadata.configs.build_config != parts[2] {
            return Err(anyhow!("Build config hash mismatch for {build_id}"));
        }

        Ok(metadata)
    }

    /// List all builds for a product
    pub fn list_builds(&self, product_code: &str) -> Result<Vec<BuildMetadata>> {
        let dir = self.data_dir.join("builds").join(product_code);

        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut builds = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry.path().extension() == Some("json".as_ref()) {
                let data = fs::read_to_string(entry.path())?;
                let metadata: BuildMetadata = serde_json::from_str(&data)?;
                builds.push(metadata);
            }
        }

        // Sort by build number (newest first)
        builds.sort_by(|a, b| b.build.build_number.cmp(&a.build.build_number));

        Ok(builds)
    }

    /// Update existing build metadata
    pub fn update_build(&self, metadata: &mut BuildMetadata) -> Result<()> {
        metadata.meta.updated_at = chrono::Utc::now();
        self.save_build(metadata)
    }

    /// Check if a build exists locally by build number
    #[must_use]
    pub fn build_exists(&self, product_code: &str, build_number: u32) -> bool {
        let path = self
            .data_dir
            .join("builds")
            .join(product_code)
            .join(format!("{build_number}.json"));

        path.exists()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_fun_call,
    clippy::unreadable_literal
)]
mod tests {
    use super::*;
    use crate::builds::metadata::{
        BuildInfo, BuildMetadata, CdnInfo, CdnProtocol, ConfigInfo, DataSource, MetadataInfo,
        ProductInfo, RegionInfo, parse_version_build,
    };
    use chrono::Utc;
    use std::collections::HashMap;
    use tempfile::TempDir;

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
                build_id: "wow_classic_era:61582:bbf06e7476382cfaa396cff0049d356b".to_string(),
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
                build_config: "bbf06e7476382cfaa396cff0049d356b".to_string(),
                cdn_config: "63eee50d456a6ddf3b630957c024dda0".to_string(),
                product_config: Some("53020d32e1a25648c8e1eafd5771935f".to_string()),
                patch_config: Some("474b9630df5b46df5d98ec27c5f78d07".to_string()),
                exe_configs,
            },
            cdn: CdnInfo {
                hosts: vec!["cdn.blizzard.com".to_string()],
                path: "tpr/wow".to_string(),
                product_path: Some("tpr/configs".to_string()),
                protocols: vec![CdnProtocol {
                    protocol: "https".to_string(),
                    port: 443,
                }],
                archive_group: Some("58a3c9e02c964b0ec9dd6c085df99a77".to_string()),
                archive_count: Some(1247),
            },
            regions,
            product: ProductInfo {
                display_name: "World of Warcraft Classic Era".to_string(),
                family: "wow".to_string(),
                product_type: "game".to_string(),
                platforms: vec!["windows".to_string()],
                subscription: Some("wow_classic".to_string()),
            },
            patch: None,
            catalog: None,
        }
    }

    fn create_test_metadata_different_build(
        build_number: u32,
        product_code: &str,
    ) -> BuildMetadata {
        let mut metadata = create_test_metadata();
        metadata.build.build_number = build_number;
        metadata.build.product_code = product_code.to_string();
        metadata.meta.build_id = format!("{product_code}:{build_number}:somehash");
        metadata
    }

    #[test]
    fn test_build_manager_creation() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = BuildManager::new(temp_dir.path());

        // Should be able to create manager with any path
        assert_eq!(manager.data_dir, temp_dir.path());
    }

    #[test]
    fn test_save_build() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = BuildManager::new(temp_dir.path());
        let metadata = create_test_metadata();

        // Save should succeed
        let result = manager.save_build(&metadata);
        assert!(result.is_ok(), "Failed to save build: {:?}", result.err());

        // File should exist with correct name (just build number)
        let expected_path = temp_dir
            .path()
            .join("builds")
            .join("wow_classic_era")
            .join("61582.json");

        assert!(
            expected_path.exists(),
            "Build file was not created at expected path"
        );

        // File should contain valid JSON
        let content = fs::read_to_string(&expected_path).expect("Failed to read build file");
        let parsed: serde_json::Value =
            serde_json::from_str(&content).expect("Build file contains invalid JSON");

        // Verify some key fields in JSON
        assert_eq!(parsed["build"]["product_code"], "wow_classic_era");
        assert_eq!(parsed["build"]["build_number"], 61582);
    }

    #[test]
    fn test_save_build_creates_directory() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = BuildManager::new(temp_dir.path());
        let metadata = create_test_metadata();

        // Ensure builds directory doesn't exist initially
        let builds_dir = temp_dir.path().join("builds");
        assert!(!builds_dir.exists());

        // Save should create necessary directories
        manager.save_build(&metadata).expect("Failed to save build");

        assert!(builds_dir.exists());
        assert!(builds_dir.join("wow_classic_era").exists());
    }

    #[test]
    fn test_load_build() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = BuildManager::new(temp_dir.path());
        let original_metadata = create_test_metadata();

        // Save first
        manager
            .save_build(&original_metadata)
            .expect("Failed to save build");

        // Load should succeed
        let loaded_metadata = manager
            .load_build("wow_classic_era", 61582)
            .expect("Failed to load build");

        // Verify loaded data matches original
        assert_eq!(
            loaded_metadata.build.product_code,
            original_metadata.build.product_code
        );
        assert_eq!(
            loaded_metadata.build.build_number,
            original_metadata.build.build_number
        );
        assert_eq!(
            loaded_metadata.build.version,
            original_metadata.build.version
        );
        assert_eq!(
            loaded_metadata.configs.build_config,
            original_metadata.configs.build_config
        );
    }

    #[test]
    fn test_load_build_not_found() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = BuildManager::new(temp_dir.path());

        // Try to load non-existent build
        let result = manager.load_build("wow_classic_era", 99999);

        assert!(result.is_err());
        let error_msg = result.expect_err("Expected error").to_string();
        assert!(error_msg.contains("Build not found"));
        assert!(error_msg.contains("wow_classic_era:99999"));
    }

    #[test]
    fn test_load_build_by_id() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = BuildManager::new(temp_dir.path());
        let metadata = create_test_metadata();

        // Save first
        manager.save_build(&metadata).expect("Failed to save build");

        // Load by build ID should succeed
        let build_id = "wow_classic_era:61582:bbf06e7476382cfaa396cff0049d356b";
        let loaded_metadata = manager
            .load_build_by_id(build_id)
            .expect("Failed to load build by ID");

        assert_eq!(loaded_metadata.build.build_number, 61582);
        assert_eq!(
            loaded_metadata.configs.build_config,
            "bbf06e7476382cfaa396cff0049d356b"
        );
    }

    #[test]
    fn test_load_build_by_id_invalid_format() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = BuildManager::new(temp_dir.path());

        let invalid_ids = vec![
            "invalid",
            "product:build",
            "product:build:hash:extra",
            "product:notanumber:hash",
        ];

        for invalid_id in invalid_ids {
            let result = manager.load_build_by_id(invalid_id);
            assert!(
                result.is_err(),
                "Expected error for invalid ID: {invalid_id}"
            );
        }
    }

    #[test]
    fn test_load_build_by_id_hash_mismatch() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = BuildManager::new(temp_dir.path());
        let metadata = create_test_metadata();

        // Save first
        manager.save_build(&metadata).expect("Failed to save build");

        // Try to load with wrong hash
        let wrong_hash_id = "wow_classic_era:61582:wronghash";
        let result = manager.load_build_by_id(wrong_hash_id);

        assert!(result.is_err());
        let error_msg = result.expect_err("Expected error").to_string();
        assert!(error_msg.contains("Build config hash mismatch"));
    }

    #[test]
    fn test_list_builds_empty() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = BuildManager::new(temp_dir.path());

        // List builds for product that doesn't exist
        let builds = manager
            .list_builds("nonexistent_product")
            .expect("Failed to list builds");

        assert!(builds.is_empty());
    }

    #[test]
    fn test_list_builds() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = BuildManager::new(temp_dir.path());

        // Save multiple builds for the same product
        let metadata1 = create_test_metadata_different_build(61582, "wow_classic_era");
        let metadata2 = create_test_metadata_different_build(61000, "wow_classic_era");
        let metadata3 = create_test_metadata_different_build(62000, "wow_classic_era");

        manager
            .save_build(&metadata1)
            .expect("Failed to save build 1");
        manager
            .save_build(&metadata2)
            .expect("Failed to save build 2");
        manager
            .save_build(&metadata3)
            .expect("Failed to save build 3");

        // List builds
        let builds = manager
            .list_builds("wow_classic_era")
            .expect("Failed to list builds");

        assert_eq!(builds.len(), 3);

        // Should be sorted by build number (newest first)
        assert_eq!(builds[0].build.build_number, 62000);
        assert_eq!(builds[1].build.build_number, 61582);
        assert_eq!(builds[2].build.build_number, 61000);
    }

    #[test]
    fn test_list_builds_different_products() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = BuildManager::new(temp_dir.path());

        // Save builds for different products
        let metadata1 = create_test_metadata_different_build(61582, "wow_classic_era");
        let metadata2 = create_test_metadata_different_build(57689, "wow");

        manager
            .save_build(&metadata1)
            .expect("Failed to save classic build");
        manager
            .save_build(&metadata2)
            .expect("Failed to save wow build");

        // List builds for each product separately
        let classic_builds = manager
            .list_builds("wow_classic_era")
            .expect("Failed to list classic builds");
        let wow_builds = manager
            .list_builds("wow")
            .expect("Failed to list wow builds");

        assert_eq!(classic_builds.len(), 1);
        assert_eq!(wow_builds.len(), 1);
        assert_eq!(classic_builds[0].build.build_number, 61582);
        assert_eq!(wow_builds[0].build.build_number, 57689);
    }

    #[test]
    fn test_build_exists() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = BuildManager::new(temp_dir.path());
        let metadata = create_test_metadata();

        // Should not exist initially
        assert!(!manager.build_exists("wow_classic_era", 61582));

        // Save build
        manager.save_build(&metadata).expect("Failed to save build");

        // Should exist now
        assert!(manager.build_exists("wow_classic_era", 61582));

        // Other builds should not exist
        assert!(!manager.build_exists("wow_classic_era", 99999));
        assert!(!manager.build_exists("other_product", 61582));
    }

    #[test]
    fn test_update_build() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = BuildManager::new(temp_dir.path());
        let mut metadata = create_test_metadata();
        let original_updated_at = metadata.meta.updated_at;

        // Save initial build
        manager.save_build(&metadata).expect("Failed to save build");

        // Wait a moment to ensure updated_at changes
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Update build
        manager
            .update_build(&mut metadata)
            .expect("Failed to update build");

        // updated_at should have changed
        assert!(metadata.meta.updated_at > original_updated_at);

        // Load and verify the updated timestamp is persisted
        let loaded_metadata = manager
            .load_build("wow_classic_era", 61582)
            .expect("Failed to load updated build");

        assert!(loaded_metadata.meta.updated_at > original_updated_at);
    }

    #[test]
    fn test_filename_format() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = BuildManager::new(temp_dir.path());

        // Test different build numbers create correctly named files
        let test_cases = vec![
            (12345, "12345.json"),
            (61582, "61582.json"),
            (1, "1.json"),
            (999999, "999999.json"),
        ];

        for (build_number, expected_filename) in test_cases {
            let metadata = create_test_metadata_different_build(build_number, "test_product");
            manager.save_build(&metadata).expect("Failed to save build");

            let expected_path = temp_dir
                .path()
                .join("builds")
                .join("test_product")
                .join(expected_filename);

            assert!(
                expected_path.exists(),
                "Expected file {expected_filename} to exist for build {build_number}"
            );
        }
    }

    #[test]
    fn test_directory_structure() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = BuildManager::new(temp_dir.path());

        // Save builds for different products
        let products = vec![
            ("wow", 57689),
            ("wow_classic", 62655),
            ("wow_classic_era", 61582),
            ("diablo4", 12345),
        ];

        for (product, build_number) in products {
            let metadata = create_test_metadata_different_build(build_number, product);
            manager
                .save_build(&metadata)
                .expect(&format!("Failed to save {product} build"));

            // Verify directory structure
            let product_dir = temp_dir.path().join("builds").join(product);
            assert!(
                product_dir.exists(),
                "Product directory should exist: {product}"
            );

            let build_file = product_dir.join(format!("{build_number}.json"));
            assert!(
                build_file.exists(),
                "Build file should exist: {}",
                build_file.display()
            );
        }

        // Verify top-level structure
        let builds_dir = temp_dir.path().join("builds");
        let entries: Vec<_> = fs::read_dir(builds_dir)
            .expect("Failed to read builds directory")
            .map(|entry| {
                entry
                    .expect("Failed to read entry")
                    .file_name()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();

        assert!(entries.contains(&"wow".to_string()));
        assert!(entries.contains(&"wow_classic".to_string()));
        assert!(entries.contains(&"wow_classic_era".to_string()));
        assert!(entries.contains(&"diablo4".to_string()));
    }

    #[test]
    fn test_parse_version_build_helper() {
        // Test the parse_version_build helper function through manager usage
        let test_cases = vec![
            ("1.15.7.61582", "1.15.7", 61582),
            ("11.0.5.57689", "11.0.5", 57689),
            ("5.4.8.18414", "5.4.8", 18414),
        ];

        for (version_build, expected_version, expected_build) in test_cases {
            let (version, build_number) = parse_version_build(version_build)
                .expect(&format!("Failed to parse {version_build}"));

            assert_eq!(version, expected_version);
            assert_eq!(build_number, expected_build);
        }
    }
}
