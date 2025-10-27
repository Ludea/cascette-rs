//! Battle.net compatible installation implementation
//!
//! Creates installations that match the official Battle.net client structure,
//! allowing games to run without modification.

use crate::{
    error::{InstallationError, Result},
    models::InstallationPlan,
};
use cascette_client_storage::{Storage, StorageConfig};
use cascette_formats::{
    archive::ArchiveIndex,
    bpsv::{BpsvBuilder, BpsvField, BpsvType, BpsvValue},
};
use std::fs;
use std::path::Path;

/// Creates a Battle.net compatible installation from an installation plan
pub struct BattlenetInstaller {
    storage: Option<Storage>,
}

impl BattlenetInstaller {
    /// Create a new Battle.net installer
    #[must_use]
    pub fn new() -> Self {
        Self { storage: None }
    }
}

impl Default for BattlenetInstaller {
    fn default() -> Self {
        Self::new()
    }
}

impl BattlenetInstaller {
    /// Initialize the Battle.net directory structure
    pub fn initialize_structure(
        &mut self,
        plan: &InstallationPlan,
        target_dir: &Path,
    ) -> Result<()> {
        println!("→ Creating Battle.net directory structure...");

        // Create main directories
        let data_dir = target_dir.join("Data");
        fs::create_dir_all(&data_dir).map_err(|e| {
            InstallationError::Other(format!("Failed to create Data directory: {e}"))
        })?;

        // Create Data subdirectories (T083d: removed residency, not in Battle.net installs)
        for subdir in &["config", "data", "indices"] {
            let path = data_dir.join(subdir);
            fs::create_dir_all(&path).map_err(|e| {
                InstallationError::Other(format!("Failed to create {subdir} directory: {e}"))
            })?;
        }

        // Create product-specific directory for WoW games only
        // Other products (agent, bna) don't use subdirectories
        if let Some(product_dir) = self.get_product_directory_name(&plan.product.code) {
            let product_path = target_dir.join(&product_dir);
            fs::create_dir_all(&product_path).map_err(|e| {
                InstallationError::Other(format!("Failed to create product directory: {e}"))
            })?;
        }

        // Initialize CASC storage system with default configuration
        let storage_config = StorageConfig {
            base_path: data_dir,
            ..Default::default()
        };

        self.storage =
            Some(Storage::new(storage_config).map_err(|e| {
                InstallationError::Other(format!("Failed to initialize storage: {e}"))
            })?);

        println!("✓ Directory structure created");
        Ok(())
    }

    /// Generate .build.info file using BPSV format
    pub fn generate_build_info(&self, plan: &InstallationPlan, target_dir: &Path) -> Result<()> {
        println!("→ Generating .build.info using BPSV builder...");

        let build_info_path = target_dir.join(".build.info");

        // Create BPSV builder with .build.info schema
        let mut builder = BpsvBuilder::new();

        // Define the schema for .build.info
        builder
            .add_field(BpsvField::new("Branch", BpsvType::String(0)))
            .add_field(BpsvField::new("Active", BpsvType::Dec(1)))
            .add_field(BpsvField::new("Build Key", BpsvType::Hex(16)))
            .add_field(BpsvField::new("CDN Key", BpsvType::Hex(16)))
            .add_field(BpsvField::new("Install Key", BpsvType::Hex(16)))
            .add_field(BpsvField::new("IM Size", BpsvType::Dec(4)))
            .add_field(BpsvField::new("CDN Path", BpsvType::String(0)))
            .add_field(BpsvField::new("CDN Hosts", BpsvType::String(0)))
            .add_field(BpsvField::new("CDN Servers", BpsvType::String(0)))
            .add_field(BpsvField::new("Tags", BpsvType::String(0)))
            .add_field(BpsvField::new("Armadillo", BpsvType::String(0)))
            .add_field(BpsvField::new("Last Activated", BpsvType::String(0)))
            .add_field(BpsvField::new("Version", BpsvType::String(0)))
            .add_field(BpsvField::new("KeyRing", BpsvType::Hex(16)))
            .add_field(BpsvField::new("Product", BpsvType::String(0)));

        // Prepare data values
        let region = &plan.product.region;
        let build_key_bytes = hex::decode(&plan.configs.build_config)
            .map_err(|e| InstallationError::Other(format!("Invalid build config hash: {e}")))?;
        let cdn_key_bytes = hex::decode(&plan.configs.cdn_config)
            .map_err(|e| InstallationError::Other(format!("Invalid CDN config hash: {e}")))?;
        let install_key_bytes = plan.manifests.install.content_key.to_vec();

        let cdn_hosts = plan.configs.cdn_hosts.join(" ");
        let cdn_servers = plan
            .configs
            .cdn_hosts
            .iter()
            .flat_map(|host| {
                vec![
                    format!("http://{}/?maxhosts=4", host),
                    format!("https://{}/?maxhosts=4&fallback=1", host),
                ]
            })
            .collect::<Vec<_>>()
            .join(" ");

        let tags = plan.target.tags.join(":");
        let version = plan.build.version();
        let product_code = &plan.product.code;

        // Add the data row
        builder
            .add_row(vec![
                BpsvValue::String(region.clone()),
                BpsvValue::Dec(1), // Active = 1
                BpsvValue::Hex(build_key_bytes),
                BpsvValue::Hex(cdn_key_bytes),
                BpsvValue::Hex(install_key_bytes),
                BpsvValue::Dec(0), // IM Size (install manifest size, not used here)
                BpsvValue::String(plan.configs.cdn_path.clone()),
                BpsvValue::String(cdn_hosts),
                BpsvValue::String(cdn_servers),
                BpsvValue::String(tags),
                BpsvValue::String(String::new()), // Armadillo (empty)
                BpsvValue::String(String::new()), // Last Activated (empty)
                BpsvValue::String(version.to_string()),
                BpsvValue::Hex(vec![]), // KeyRing (empty)
                BpsvValue::String(product_code.clone()),
            ])
            .map_err(|e| InstallationError::Other(format!("Failed to add row to BPSV: {e}")))?;

        // Build and format the document
        let document = builder.build();
        let content = cascette_formats::bpsv::format(&document);

        // Write to file
        fs::write(&build_info_path, content)
            .map_err(|e| InstallationError::Other(format!("Failed to write .build.info: {e}")))?;

        println!("✓ .build.info generated with BPSV builder");
        Ok(())
    }

    /// Generate .product.db file (Battle.net compatible protobuf)
    pub fn generate_product_db(&self, plan: &InstallationPlan, target_dir: &Path) -> Result<()> {
        println!("→ Generating .product.db with complete protobuf data...");

        let product_db_path = target_dir.join(".product.db");
        let product_name = &plan.product.code;
        let version = plan.build.version();
        let build_key = &plan.configs.build_config;
        let region = &plan.product.region;

        // Create protobuf data matching Battle.net structure
        let mut data = Vec::new();

        // Field 1: Product code (string)
        data.push(0x0a);
        data.push(product_name.len() as u8);
        data.extend_from_slice(product_name.as_bytes());

        // Field 2: Product name (string) - same as code for Classic
        data.push(0x12);
        data.push(product_name.len() as u8);
        data.extend_from_slice(product_name.as_bytes());

        // Field 3: Install info structure
        data.push(0x1a);
        let locale = self.extract_locale_from_tags(&plan.target.tags);
        let install_info = self.build_install_info_protobuf(target_dir, region, &locale);
        data.push(install_info.len() as u8);
        data.extend(install_info);

        // Field 4: Build info structure
        data.push(0x22);
        let build_info = self.build_build_info_protobuf(version, build_key);
        data.extend_from_slice(&self.encode_varint(build_info.len()));
        data.extend(build_info);

        fs::write(&product_db_path, data)
            .map_err(|e| InstallationError::Other(format!("Failed to write .product.db: {e}")))?;

        println!("✓ .product.db generated with complete protobuf data");
        Ok(())
    }

    /// Build install info protobuf structure
    fn build_install_info_protobuf(
        &self,
        target_dir: &Path,
        region: &str,
        locale: &str,
    ) -> Vec<u8> {
        let mut data = Vec::new();

        // Field 3: Install path (string)
        let install_path = target_dir.to_string_lossy();
        data.push(0x1a);
        data.push(install_path.len() as u8);
        data.extend_from_slice(install_path.as_bytes());

        // Field 2: Region (string)
        data.push(0x12);
        data.push(region.len() as u8);
        data.extend_from_slice(region.as_bytes());

        // Field 3: Unknown field (int32)
        data.push(0x18);
        data.push(0x02); // Value 2

        // Field 4: Unknown field (int32)
        data.push(0x20);
        data.push(0x02); // Value 2

        // Field 5: Unknown field (int32)
        data.push(0x28);
        data.push(0x03); // Value 3

        // Field 6: Locale info
        data.push(0x32);
        data.push(locale.len() as u8);
        data.extend_from_slice(locale.as_bytes());

        // Field 7: Locale info repeated
        data.push(0x3a);
        data.push(locale.len() as u8);
        data.extend_from_slice(locale.as_bytes());

        // Field 8: Settings structure
        data.push(0x42);
        data.push((2 + locale.len()) as u8); // Length
        data.push(0x0a); // Field 1
        data.push(locale.len() as u8);
        data.extend_from_slice(locale.as_bytes());
        data.push(0x10); // Field 2
        data.push(0x03); // Value 3

        data
    }

    /// Build build info protobuf structure
    fn build_build_info_protobuf(&self, version: &str, build_key: &str) -> Vec<u8> {
        let mut data = vec![
            // Field 1: Some flag (int32)
            0x08,
            0x01, // Value 1
            // Field 2: Some flag (int32)
            0x10,
            0x01, // Value 1
            // Field 3: Some flag (int32)
            0x18,
            0x01, // Value 1
            // Field 4: Some flag (int32)
            0x20,
            0x00, // Value 0
            // Field 5: Some flag (int32)
            0x28,
            0x01, // Value 1
            // Field 7: Version string
            0x3a,
            version.len() as u8,
        ];

        data.extend_from_slice(version.as_bytes());

        // Field 12: Build key (bytes)
        data.push(0x62);
        data.push(0x20); // 32 bytes for build key
        data.extend_from_slice(build_key.as_bytes());

        // Field 14: Build key repeated (bytes)
        data.push(0x72);
        data.push(0x20); // 32 bytes
        data.extend_from_slice(build_key.as_bytes());

        // Field 16: Content key (bytes)
        data.push(0x82);
        data.push(0x01); // Varint length prefix
        data.push(0x20); // 32 bytes
        let content_key = "5090256c2742e6652de8aef3641c6eb1"; // From hexdump
        data.extend_from_slice(content_key.as_bytes());

        data
    }

    /// Encode varint for protobuf
    fn encode_varint(&self, mut value: usize) -> Vec<u8> {
        let mut result = Vec::new();
        loop {
            let mut byte = (value & 0x7F) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            result.push(byte);
            if value == 0 {
                break;
            }
        }
        result
    }

    /// Generate other metadata files
    pub fn generate_metadata_files(
        &self,
        plan: &InstallationPlan,
        target_dir: &Path,
    ) -> Result<()> {
        println!("→ Generating metadata files...");

        // .patch.result - indicates successful patch
        let patch_result_path = target_dir.join(".patch.result");
        fs::write(&patch_result_path, vec![0x01])
            .map_err(|e| InstallationError::Other(format!("Failed to write .patch.result: {e}")))?;

        // Launcher.db - contains locale setting
        let locale = self.extract_locale_from_tags(&plan.target.tags);
        let launcher_db_path = target_dir.join("Launcher.db");
        fs::write(&launcher_db_path, locale.as_bytes())
            .map_err(|e| InstallationError::Other(format!("Failed to write Launcher.db: {e}")))?;

        // .flavor.info in product directory (WoW games only)
        if let Some(product_dir) = self.get_product_directory_name(&plan.product.code) {
            let flavor_info_path = target_dir.join(&product_dir).join(".flavor.info");

            let flavor_content = format!("Product Flavor!STRING:0\n{}\n", plan.product.code);
            fs::write(&flavor_info_path, flavor_content).map_err(|e| {
                InstallationError::Other(format!("Failed to write .flavor.info: {e}"))
            })?;
        }

        println!("✓ Metadata files generated");
        Ok(())
    }

    /// Store CDN configuration files in the proper structure
    pub fn store_cdn_configs(
        &self,
        plan: &InstallationPlan,
        target_dir: &Path,
        cdn_config_data: &[u8],
        build_config_data: &[u8],
    ) -> Result<()> {
        println!("→ Storing CDN configuration files...");

        let config_dir = target_dir.join("Data").join("config");

        // Store CDN config
        let cdn_config_hash_bytes = hex::decode(&plan.configs.cdn_config)
            .map_err(|e| InstallationError::Other(format!("Invalid CDN config hash: {e}")))?;
        self.store_config_file(&config_dir, &cdn_config_hash_bytes, cdn_config_data)?;

        // Store build config
        let build_config_hash_bytes = hex::decode(&plan.configs.build_config)
            .map_err(|e| InstallationError::Other(format!("Invalid build config hash: {e}")))?;
        self.store_config_file(&config_dir, &build_config_hash_bytes, build_config_data)?;

        println!("✓ Configuration files stored");
        Ok(())
    }

    /// Store a configuration file using the hash-based directory structure
    fn store_config_file(&self, config_dir: &Path, hash: &[u8], data: &[u8]) -> Result<()> {
        let hash_hex = hex::encode(hash);

        // Create directory structure: config/{hash[0:2]}/{hash[2:4]}/
        let dir_path = config_dir.join(&hash_hex[0..2]).join(&hash_hex[2..4]);

        fs::create_dir_all(&dir_path).map_err(|e| {
            InstallationError::Other(format!("Failed to create config directory: {e}"))
        })?;

        // Write file with full hash as filename
        let file_path = dir_path.join(&hash_hex);
        fs::write(&file_path, data)
            .map_err(|e| InstallationError::Other(format!("Failed to write config file: {e}")))?;

        Ok(())
    }

    /// Get the product-specific directory name (`WoW` games only)
    /// Returns None for non-`WoW` products (agent, bna, etc.)
    fn get_product_directory_name(&self, product_code: &str) -> Option<String> {
        match product_code {
            "wow" => Some("_retail_".to_string()),
            "wow_classic" => Some("_classic_".to_string()),
            "wow_classic_era" => Some("_classic_era_".to_string()),
            "wow_classic_ptr" => Some("_classic_ptr_".to_string()),
            "wowt" | "wow_beta" => Some("_ptr_".to_string()),
            // Other products (agent, bna, etc.) don't use subdirectories
            _ => None,
        }
    }

    /// Extract locale from tags (e.g., "enUS", "deDE", "frFR")
    fn extract_locale_from_tags(&self, tags: &[String]) -> String {
        for tag in tags {
            // Locale tags are typically 4 characters: language + country code
            if tag.len() == 4
                && tag.chars().take(2).all(|c| c.is_ascii_lowercase())
                && tag.chars().skip(2).all(|c| c.is_ascii_uppercase())
            {
                return tag.clone();
            }
        }
        // Fallback to English US if no locale found
        "enUS".to_string()
    }

    /// Generate local .idx files for content addressing
    pub fn generate_local_indices(
        &mut self,
        plan: &InstallationPlan,
        target_dir: &Path,
    ) -> Result<()> {
        println!("→ Generating local .idx files...");

        let data_dir = target_dir.join("Data").join("data");

        // Determine CASC version based on build number
        // Pre-BfA builds (< 8.0, build < 26000) use older format
        let is_legacy_format = plan.build.build_id() < 26000;

        if is_legacy_format {
            // Legacy format for Classic 1.x (192KB files, no IDX magic)
            self.generate_legacy_idx_files(&data_dir, plan)?;
        } else {
            // Modern format (320KB files with IDX header)
            self.generate_modern_idx_files(&data_dir)?;
        }

        println!("✓ Local .idx files generated");
        Ok(())
    }

    /// Generate legacy .idx files (pre-BfA format)
    fn generate_legacy_idx_files(&self, data_dir: &Path, _plan: &InstallationPlan) -> Result<()> {
        // Legacy format uses smaller files and different naming
        // Example: 000000000a.idx, 010000000d.idx
        // Size: 192KB (196,608 bytes)

        // For now, create minimal legacy format files
        // In full implementation, would parse archive indices and build proper mappings
        let legacy_size = 196_608;

        // Create a few sample .idx files matching the pattern
        for version in 0x0a..=0x0e {
            for bucket in 0..5 {
                let filename = format!("{bucket:02x}000000{version:02x}.idx");
                let idx_path = data_dir.join(&filename);

                let mut idx_data = Vec::new();

                // Legacy format header (simplified)
                idx_data.extend_from_slice(&[0x10, 0x00, 0x00, 0x00]); // Header size
                idx_data.extend_from_slice(&[0x22, 0xab, 0x86, 0x42]); // Unknown field
                idx_data.extend_from_slice(&[0x07, 0x00, 0x00, 0x00]); // Version?
                idx_data.extend_from_slice(&[0x04, 0x05, 0x09, 0x1e]); // Unknown field

                // Pad to legacy size
                idx_data.resize(legacy_size, 0);

                fs::write(&idx_path, idx_data).map_err(|e| {
                    InstallationError::Other(format!(
                        "Failed to write legacy .idx file {filename}: {e}"
                    ))
                })?;
            }
        }

        Ok(())
    }

    /// Generate modern .idx files (`BfA`+ format)
    fn generate_modern_idx_files(&self, data_dir: &Path) -> Result<()> {
        // Create 16 index buckets (00-0f)
        for bucket in 0..16 {
            // Format: {bucket:02x}000000{version:02x}.idx
            let filename = format!("{bucket:02x}0000001f.idx");
            let idx_path = data_dir.join(&filename);

            let mut idx_data = Vec::new();

            // IDX header
            idx_data.extend_from_slice(b"IDX\x01"); // Magic and version
            idx_data.extend_from_slice(&[0; 4]); // Bucket index
            idx_data.extend_from_slice(&[0; 4]); // Used size
            idx_data.extend_from_slice(&[0; 4]); // File size

            // Pad to standard size (320KB)
            idx_data.resize(327_680, 0);

            fs::write(&idx_path, idx_data).map_err(|e| {
                InstallationError::Other(format!("Failed to write .idx file {filename}: {e}"))
            })?;
        }

        Ok(())
    }

    /// Generate archive-group index by merging all archive indices
    /// This creates a client-generated mega-index with special 6-byte offset format
    pub fn generate_archive_group(
        &self,
        _plan: &InstallationPlan,
        target_dir: &Path,
    ) -> Result<()> {
        use std::collections::HashMap;

        println!("→ Generating archive-group mega-index...");

        let indices_dir = target_dir.join("Data").join("indices");

        // Scan for all .index files in the directory
        // Only process archive indices (38 chars = 32 hex + 6 ".index")
        // Skip existing archive-groups which may be larger
        let index_files = fs::read_dir(&indices_dir)
            .map_err(|e| {
                InstallationError::Other(format!("Failed to read indices directory: {e}"))
            })?
            .filter_map(std::result::Result::ok)
            .filter(|entry| {
                if let Some(name) = entry.file_name().to_str() {
                    // Only process normal archive indices (not archive-groups)
                    // Archive indices are exactly 38 chars, archive-groups may vary
                    std::path::Path::new(name)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("index"))
                        && name.len() == 38
                } else {
                    false
                }
            })
            .filter(|entry| {
                // Additional filter: skip files larger than 1MB (likely archive-groups)
                if let Ok(metadata) = entry.metadata() {
                    metadata.len() < 1024 * 1024 // Archive indices are typically <1MB
                } else {
                    true
                }
            })
            .collect::<Vec<_>>();

        println!(
            "  → Found {} archive index files to process",
            index_files.len()
        );

        // Use a HashMap to deduplicate entries by encoding key
        // When duplicates are found, keep the first occurrence
        let mut unique_entries: HashMap<Vec<u8>, (Vec<u8>, u32)> = HashMap::new();

        for (archive_idx, index_file_entry) in index_files.iter().enumerate() {
            let index_file_path = index_file_entry.path();
            let file_name = index_file_entry.file_name();
            let hash_str = file_name
                .to_str()
                .expect("Invalid UTF-8 in filename")
                .strip_suffix(".index")
                .expect("Filename should end with .index");

            // Read and parse the archive index
            let index_data = fs::read(&index_file_path).map_err(|e| {
                InstallationError::Other(format!("Failed to read index file {hash_str}: {e}"))
            })?;

            let mut cursor = std::io::Cursor::new(index_data);
            match ArchiveIndex::parse(&mut cursor) {
                Ok(index) => {
                    // Add each entry with the archive index embedded in the offset
                    for entry in index.entries {
                        // Only add if not already present (deduplication)
                        if !unique_entries.contains_key(&entry.encoding_key) {
                            // Create a 6-byte offset: 2 bytes archive index + 4 bytes offset
                            let mut combined_offset = Vec::new();
                            combined_offset.extend_from_slice(&(archive_idx as u16).to_be_bytes());
                            combined_offset.extend_from_slice(&(entry.offset as u32).to_be_bytes());

                            unique_entries
                                .insert(entry.encoding_key.clone(), (combined_offset, entry.size));
                        }
                    }
                }
                Err(e) => {
                    println!("  ⚠ Skipping unparseable index file {hash_str}: {e}");
                }
            }
        }

        println!(
            "  → Deduplicated to {} unique entries from {} archives",
            unique_entries.len(),
            index_files.len()
        );

        // Convert HashMap to Vec and sort by encoding key
        let mut all_entries: Vec<(Vec<u8>, Vec<u8>, u32)> = unique_entries
            .into_iter()
            .map(|(key, (offset, size))| (key, offset, size))
            .collect();

        all_entries.sort_by(|a, b| a.0.cmp(&b.0));

        // Build archive-group using standard index format with 6-byte offsets
        // We need to build this manually since ArchiveIndexBuilder assumes 4-byte offsets
        let mut archive_group_data = Vec::new();

        // Write entries directly (no custom header - standard index format)
        for (encoding_key, combined_offset, size) in &all_entries {
            // Write encoding key (typically 16 bytes)
            archive_group_data.extend_from_slice(encoding_key);
            // Write 6-byte combined offset (archive index + offset)
            archive_group_data.extend_from_slice(combined_offset);
            // Write 4-byte size
            archive_group_data.extend_from_slice(&size.to_be_bytes());
        }

        // Calculate and append footer (standard index footer with offsetBytes=6)
        let footer_data = self.build_archive_group_footer(all_entries.len());
        archive_group_data.extend_from_slice(&footer_data);

        // Calculate hash of the complete archive-group data
        let mega_index_hash = md5::compute(&archive_group_data);
        let mega_index_hash_hex = format!("{mega_index_hash:x}");

        // Store the archive-group file
        let mega_index_path = indices_dir.join(format!("{mega_index_hash_hex}.index"));
        fs::write(&mega_index_path, &archive_group_data)
            .map_err(|e| InstallationError::Other(format!("Failed to write archive-group: {e}")))?;

        println!("✓ Archive-group mega-index generated: {mega_index_hash_hex}");
        println!(
            "  Size: {} bytes, Entries: {}",
            archive_group_data.len(),
            all_entries.len()
        );
        Ok(())
    }

    /// Build standard index footer for archive-group (with 6-byte offsets)
    fn build_archive_group_footer(&self, entry_count: usize) -> Vec<u8> {
        let mut footer = Vec::new();

        // Standard CDN index footer format for archive-groups
        // Must match Battle.net's exact format for binary compatibility

        // TOC hash (8 bytes) - leave empty for archive-groups
        footer.extend_from_slice(&[0; 8]);

        // Version and flags (4 bytes)
        footer.push(0x01); // Version byte 1
        footer.push(0x00); // Version byte 2
        footer.push(0x00); // Version byte 3
        footer.push(0x04); // Version byte 4

        // Index configuration
        footer.push(0x06); // offset_bytes (6 bytes for archive-group!)
        footer.push(0x04); // size_bytes (4 bytes)
        footer.push(0x10); // key_bytes (16 bytes for encoding keys)
        footer.push(0x08); // hash_bytes (8 bytes for Jenkins96 hash)

        // Entry count (4 bytes, little-endian)
        footer.extend_from_slice(&(entry_count as u32).to_le_bytes());

        // Calculate MD5 hash of footer (excluding the hash itself)
        // Footer hash is calculated over the 20-byte fixed portion
        let content_key = cascette_crypto::md5::ContentKey::from_data(&footer);
        let footer_hash = content_key.as_bytes()[..8].to_vec();
        footer.extend_from_slice(&footer_hash);

        footer
    }
}
