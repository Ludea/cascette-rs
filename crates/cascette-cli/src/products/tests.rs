//! Comprehensive tests for enhanced info command functionality

#[cfg(test)]
#[allow(clippy::expect_fun_call, clippy::panic, clippy::unwrap_used, dead_code)]
mod info_tests {
    use super::super::helper_functions;
    use crate::installation::builds::BuildManager;
    use crate::installation::builds::metadata::{
        BuildInfo, BuildMetadata, CdnInfo, CdnProtocol, ConfigInfo, DataSource, MetadataInfo,
        ProductInfo, RegionInfo,
    };
    use chrono::Utc;
    use std::collections::HashMap;
    use tempfile::TempDir;

    /// Create test build metadata for testing
    fn create_test_build_metadata(
        product_code: &str,
        build_number: u32,
        version: &str,
    ) -> BuildMetadata {
        let now = Utc::now();
        let mut regions = HashMap::new();
        regions.insert(
            "us".to_string(),
            RegionInfo {
                region: "us".to_string(),
                available: true,
                cdn_hosts: None,
                version_string: Some(format!("{}.{}", version, build_number)),
            },
        );

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
                build_id: format!("{}:{}:testhash", product_code, build_number),
            },
            build: BuildInfo {
                product_code: product_code.to_string(),
                version: version.to_string(),
                build_number,
                version_build: format!("{}.{}", version, build_number),
                build_name: Some(format!("TEST-{}-{}", build_number, version)),
                build_uid: Some(product_code.to_string()),
                build_product: Some("Test Product".to_string()),
                branch: Some("test".to_string()),
            },
            configs: ConfigInfo {
                build_config: "testhash123".to_string(),
                cdn_config: "cdnhash456".to_string(),
                product_config: Some("producthash789".to_string()),
                patch_config: None,
                exe_configs: HashMap::new(),
            },
            cdn: CdnInfo {
                hosts: vec!["test-cdn.example.com".to_string()],
                path: "test/path".to_string(),
                product_path: Some("test/config".to_string()),
                protocols: vec![CdnProtocol {
                    protocol: "https".to_string(),
                    port: 443,
                }],
                archive_group: Some("testgroup".to_string()),
                archive_count: Some(100),
            },
            regions,
            product: ProductInfo {
                display_name: "Test Product".to_string(),
                family: "test".to_string(),
                product_type: "game".to_string(),
                platforms: vec!["test".to_string()],
                subscription: None,
            },
            patch: None,
            catalog: None,
        }
    }

    /// Test that cached build info loading works correctly
    #[test]
    fn test_handle_cached_build_info_success() {
        // Create temporary directory for build manager
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let build_manager = BuildManager::new(temp_dir.path());

        // Create and save test build metadata
        let metadata = create_test_build_metadata("wow_classic_era", 61582, "1.15.7");
        build_manager
            .save_build(&metadata)
            .expect("Failed to save test build");

        // Test loading cached build info - this should succeed without network calls
        let result =
            helper_functions::handle_cached_build_info(&build_manager, "wow_classic_era", 61582);

        assert!(result.is_ok(), "Should successfully load cached build info");
    }

    /// Test that loading non-existent build returns appropriate error
    #[test]
    fn test_handle_cached_build_info_not_found() {
        // Create temporary directory for build manager
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let build_manager = BuildManager::new(temp_dir.path());

        // Test loading non-existent build - should return error
        let result = helper_functions::handle_cached_build_info(
            &build_manager,
            "wow_classic_era",
            99999, // Non-existent build
        );

        assert!(
            result.is_err(),
            "Should return error for non-existent build"
        );
        let error_msg = result
            .expect_err("Result should be an error as verified above")
            .to_string();
        assert!(error_msg.contains("Build not cached locally"));
    }

    /// Test that displaying cached build info doesn't panic
    #[test]
    fn test_display_cached_build_info() {
        let metadata = create_test_build_metadata("wow_classic_era", 61582, "1.15.7");

        // This test just verifies the function doesn't panic
        // In a real implementation, we might capture stdout to verify output format
        let result = helper_functions::display_cached_build_info(&metadata);
        assert!(result.is_ok(), "Should successfully display build info");
    }

    /// Test displaying cached builds with empty list
    #[test]
    fn test_display_cached_builds_empty() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let build_manager = BuildManager::new(temp_dir.path());

        // Test with no cached builds
        let result = helper_functions::display_cached_builds(&build_manager, "nonexistent_product");
        assert!(result.is_ok(), "Should handle empty build list gracefully");
    }

    /// Test displaying cached builds with multiple builds
    #[test]
    fn test_display_cached_builds_with_builds() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let build_manager = BuildManager::new(temp_dir.path());

        // Save multiple test builds
        let builds = vec![
            create_test_build_metadata("wow_classic_era", 61582, "1.15.7"),
            create_test_build_metadata("wow_classic_era", 61000, "1.15.6"),
            create_test_build_metadata("wow_classic_era", 62000, "1.15.8"),
        ];

        for build in &builds {
            build_manager
                .save_build(build)
                .expect("Failed to save test build");
        }

        let result = helper_functions::display_cached_builds(&build_manager, "wow_classic_era");
        assert!(result.is_ok(), "Should successfully display cached builds");
    }

    /// Test the build manager integration
    #[test]
    fn test_build_manager_integration() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let build_manager = BuildManager::new(temp_dir.path());

        // Test build exists check
        assert!(!build_manager.build_exists("test_product", 12345));

        // Save a build
        let metadata = create_test_build_metadata("test_product", 12345, "1.0.0");
        build_manager
            .save_build(&metadata)
            .expect("Failed to save build");

        // Check it exists now
        assert!(build_manager.build_exists("test_product", 12345));

        // Load it back
        let loaded = build_manager
            .load_build("test_product", 12345)
            .expect("Failed to load build");

        assert_eq!(loaded.build.product_code, "test_product");
        assert_eq!(loaded.build.build_number, 12345);
        assert_eq!(loaded.build.version, "1.0.0");
    }

    /// Test build metadata serialization roundtrip
    #[test]
    fn test_build_metadata_serialization_roundtrip() {
        let metadata = create_test_build_metadata("test_product", 12345, "1.0.0");

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&metadata).expect("Failed to serialize metadata");

        // Verify JSON contains expected fields
        assert!(json.contains("test_product"));
        assert!(json.contains("12345"));
        assert!(json.contains("1.0.0"));

        // Deserialize back
        let deserialized: BuildMetadata =
            serde_json::from_str(&json).expect("Failed to deserialize metadata");

        // Verify key fields match
        assert_eq!(deserialized.build.product_code, metadata.build.product_code);
        assert_eq!(deserialized.build.build_number, metadata.build.build_number);
        assert_eq!(deserialized.build.version, metadata.build.version);
    }

    /// Test parse_version_build integration
    #[test]
    fn test_parse_version_build_integration() {
        let test_cases = vec![
            ("1.15.7.61582", "1.15.7", 61582),
            ("11.0.5.57689", "11.0.5", 57689),
            ("10.2.7.54577", "10.2.7", 54577),
        ];

        for (input, expected_version, expected_build) in test_cases {
            let result = crate::installation::builds::parse_version_build(input);
            assert!(result.is_ok(), "Failed to parse {}", input);

            let (version, build_number) =
                result.expect("Version parsing should succeed for valid input");
            assert_eq!(version, expected_version);
            assert_eq!(build_number, expected_build);
        }
    }

    /// Test parse_version_build with invalid input
    #[test]
    fn test_parse_version_build_invalid() {
        let invalid_cases = vec![
            "1.15.7",     // Missing build number
            "1.15",       // Too few parts
            "invalid",    // Not version format
            "1.15.7.abc", // Non-numeric build
            "",           // Empty string
        ];

        for invalid_input in invalid_cases {
            let result = crate::installation::builds::parse_version_build(invalid_input);
            assert!(result.is_err(), "Expected error for: {}", invalid_input);
        }
    }

    /// Test the complete build caching workflow
    #[test]
    fn test_build_caching_workflow() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let build_manager = BuildManager::new(temp_dir.path());

        // Simulate the workflow:
        // 1. Check if build exists (should be false initially)
        assert!(!build_manager.build_exists("wow_classic_era", 61582));

        // 2. "Query" live NGDP and create metadata (simulated)
        let metadata = create_test_build_metadata("wow_classic_era", 61582, "1.15.7");

        // 3. Save the metadata
        build_manager
            .save_build(&metadata)
            .expect("Failed to save build");

        // 4. Check that it exists now
        assert!(build_manager.build_exists("wow_classic_era", 61582));

        // 5. Load it back (simulate --build parameter)
        let loaded = build_manager
            .load_build("wow_classic_era", 61582)
            .expect("Failed to load cached build");

        // 6. Verify the loaded data matches what we saved
        assert_eq!(loaded.build.product_code, "wow_classic_era");
        assert_eq!(loaded.build.build_number, 61582);
        assert_eq!(loaded.build.version, "1.15.7");
        assert_eq!(loaded.configs.build_config, "testhash123");

        // 7. List builds to verify it appears in the list
        let builds = build_manager
            .list_builds("wow_classic_era")
            .expect("Failed to list builds");

        assert_eq!(builds.len(), 1);
        assert_eq!(builds[0].build.build_number, 61582);
    }

    /// Test multiple product build isolation
    #[test]
    fn test_multiple_product_build_isolation() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let build_manager = BuildManager::new(temp_dir.path());

        // Save builds for different products
        let wow_metadata = create_test_build_metadata("wow", 57689, "11.0.5");
        let classic_metadata = create_test_build_metadata("wow_classic_era", 61582, "1.15.7");

        build_manager
            .save_build(&wow_metadata)
            .expect("Failed to save wow build");
        build_manager
            .save_build(&classic_metadata)
            .expect("Failed to save classic build");

        // Verify each product only sees its own builds
        let wow_builds = build_manager
            .list_builds("wow")
            .expect("Failed to list wow builds");
        let classic_builds = build_manager
            .list_builds("wow_classic_era")
            .expect("Failed to list classic builds");

        assert_eq!(wow_builds.len(), 1);
        assert_eq!(classic_builds.len(), 1);
        assert_eq!(wow_builds[0].build.build_number, 57689);
        assert_eq!(classic_builds[0].build.build_number, 61582);

        // Verify cross-product lookups fail appropriately
        assert!(!build_manager.build_exists("wow", 61582));
        assert!(!build_manager.build_exists("wow_classic_era", 57689));
    }

    /// Test different data source types
    #[test]
    fn test_data_source_types() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let build_manager = BuildManager::new(temp_dir.path());

        // Test different data source types
        let now = Utc::now();

        // Live data source
        let mut live_metadata = create_test_build_metadata("test_product", 1, "1.0.0");
        live_metadata.meta.source = DataSource::Live {
            region: "us".to_string(),
            endpoint: "us.version.battle.net".to_string(),
            query_time: now,
        };

        // Wago data source
        let mut wago_metadata = create_test_build_metadata("test_product", 2, "1.0.1");
        wago_metadata.meta.source = DataSource::Wago {
            import_time: now,
            wago_version_id: Some("wago123".to_string()),
        };

        // Manual data source
        let mut manual_metadata = create_test_build_metadata("test_product", 3, "1.0.2");
        manual_metadata.meta.source = DataSource::Manual {
            created_by: "test_user".to_string(),
            reason: "Testing".to_string(),
        };

        // Save all builds
        build_manager
            .save_build(&live_metadata)
            .expect("Failed to save live build");
        build_manager
            .save_build(&wago_metadata)
            .expect("Failed to save wago build");
        build_manager
            .save_build(&manual_metadata)
            .expect("Failed to save manual build");

        // Load them back and verify data sources are preserved
        let loaded_live = build_manager
            .load_build("test_product", 1)
            .expect("Failed to load live build");
        let loaded_wago = build_manager
            .load_build("test_product", 2)
            .expect("Failed to load wago build");
        let loaded_manual = build_manager
            .load_build("test_product", 3)
            .expect("Failed to load manual build");

        // Verify data sources are correct
        match &loaded_live.meta.source {
            DataSource::Live {
                region, endpoint, ..
            } => {
                assert_eq!(region, "us");
                assert_eq!(endpoint, "us.version.battle.net");
            }
            _ => panic!("Expected Live data source"),
        }

        match &loaded_wago.meta.source {
            DataSource::Wago {
                wago_version_id, ..
            } => {
                assert_eq!(wago_version_id, &Some("wago123".to_string()));
            }
            _ => panic!("Expected Wago data source"),
        }

        match &loaded_manual.meta.source {
            DataSource::Manual { created_by, reason } => {
                assert_eq!(created_by, "test_user");
                assert_eq!(reason, "Testing");
            }
            _ => panic!("Expected Manual data source"),
        }
    }

    /// Test that build numbers are properly extracted from version strings
    #[test]
    fn test_build_number_extraction() {
        // Test the core logic for extracting build numbers from versions
        // This would require a proper BpsvDocument implementation

        let test_cases = vec![
            ("1.15.7.61582", 61582),
            ("11.0.5.57689", 57689),
            ("10.2.7.54577", 54577),
        ];

        for (version_string, expected_build) in test_cases {
            if let Ok((_, build_number)) =
                crate::installation::builds::parse_version_build(version_string)
            {
                assert_eq!(build_number, expected_build);
            } else {
                panic!("Failed to parse version string: {}", version_string);
            }
        }
    }

    /// Test that directory structure is correctly maintained
    #[test]
    fn test_directory_structure() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let build_manager = BuildManager::new(temp_dir.path());

        // Save builds for different products
        let products = vec![
            ("wow", 57689, "11.0.5"),
            ("wow_classic", 62655, "5.4.8"),
            ("wow_classic_era", 61582, "1.15.7"),
            ("d4", 12345, "1.0.0"),
        ];

        for (product, build_number, version) in products {
            let metadata = create_test_build_metadata(product, build_number, version);
            build_manager
                .save_build(&metadata)
                .unwrap_or_else(|_| panic!("Failed to save {} build", product));
        }

        // Verify directory structure
        let builds_dir = temp_dir.path().join("builds");
        assert!(builds_dir.exists());

        // Check each product directory exists
        for (product, build_number, _) in [
            ("wow", 57689, "11.0.5"),
            ("wow_classic", 62655, "5.4.8"),
            ("wow_classic_era", 61582, "1.15.7"),
            ("d4", 12345, "1.0.0"),
        ] {
            let product_dir = builds_dir.join(product);
            assert!(
                product_dir.exists(),
                "Product directory should exist: {}",
                product
            );

            let build_file = product_dir.join(format!("{}.json", build_number));
            assert!(
                build_file.exists(),
                "Build file should exist: {}",
                build_file.display()
            );
        }
    }

    /// Test error handling for corrupted build files
    #[test]
    fn test_corrupted_build_file_handling() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let build_manager = BuildManager::new(temp_dir.path());

        // Create directory structure
        let product_dir = temp_dir.path().join("builds").join("test_product");
        std::fs::create_dir_all(&product_dir).expect("Failed to create directory");

        // Write corrupted JSON file
        let build_file = product_dir.join("12345.json");
        std::fs::write(&build_file, "invalid json content").expect("Failed to write file");

        // Try to load the corrupted build
        let result = build_manager.load_build("test_product", 12345);
        assert!(result.is_err(), "Should fail to load corrupted build file");
    }

    /// Test loading builds with missing optional fields
    #[test]
    fn test_optional_fields_handling() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let build_manager = BuildManager::new(temp_dir.path());

        // Create metadata with minimal fields
        let mut metadata = create_test_build_metadata("test_product", 12345, "1.0.0");
        metadata.build.build_name = None;
        metadata.build.branch = None;
        metadata.configs.product_config = None;
        metadata.patch = None;
        metadata.catalog = None;

        // Save and reload
        build_manager
            .save_build(&metadata)
            .expect("Failed to save minimal build");
        let loaded = build_manager
            .load_build("test_product", 12345)
            .expect("Failed to load minimal build");

        // Verify optional fields are None
        assert!(loaded.build.build_name.is_none());
        assert!(loaded.build.branch.is_none());
        assert!(loaded.configs.product_config.is_none());
        assert!(loaded.patch.is_none());
        assert!(loaded.catalog.is_none());
    }
}
