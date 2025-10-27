//! TACT key manager wrapper for CLI operations
//!
//! This module provides a thin wrapper around cascette-metadata's TACT key
//! manager, maintaining the CLI's existing interface while delegating all
//! orchestration to the metadata layer.

use anyhow::Result;
use cascette_metadata::TactKeyManager as MetadataTactKeyManager;
use std::fmt::Write;
use std::path::Path;

// Re-export types from metadata for compatibility
pub use cascette_metadata::TactKeyStats;

/// CLI wrapper for TACT key management
///
/// This wrapper delegates all operations to cascette-metadata's TactKeyManager
/// while maintaining the CLI's existing interface.
pub struct TactKeyManager {
    /// The underlying metadata manager
    inner: MetadataTactKeyManager,
}

impl TactKeyManager {
    /// Create a new TACT key manager with the given data directory
    pub fn new(data_dir: impl AsRef<Path>) -> Result<Self> {
        let inner = MetadataTactKeyManager::new(data_dir)?;
        Ok(Self { inner })
    }

    /// Add a new TACT key
    pub fn add_key(
        &mut self,
        key_id: u64,
        key_hex: &str,
        source: &str,
        description: Option<String>,
        product: Option<String>,
        build: Option<u32>,
    ) -> Result<()> {
        self.inner
            .add_key(key_id, key_hex, source, description, product, build)?;
        Ok(())
    }

    /// Add a new TACT key for batch operations
    pub fn add_key_batch(
        &mut self,
        key_id: u64,
        key_hex: &str,
        source: &str,
        description: Option<String>,
        product: Option<String>,
        build: Option<u32>,
    ) -> Result<()> {
        self.inner
            .add_key_batch(key_id, key_hex, source, description, product, build)?;
        Ok(())
    }

    /// Add a new verified TACT key for batch operations
    #[allow(dead_code)]
    pub fn add_verified_key_batch(
        &mut self,
        key_id: u64,
        key_hex: &str,
        source: &str,
        description: Option<String>,
        product: Option<String>,
        build: Option<u32>,
    ) -> Result<()> {
        self.inner
            .add_verified_key_batch(key_id, key_hex, source, description, product, build)?;
        Ok(())
    }

    /// Get a TACT key by ID
    pub fn get_key(
        &mut self,
        key_id: u64,
    ) -> Result<
        Option<(
            cascette_crypto::TactKey,
            Option<cascette_crypto::KeyMetadata>,
        )>,
    > {
        Ok(self.inner.get_key(key_id)?)
    }

    /// Remove a TACT key
    pub fn remove_key(&mut self, key_id: u64) -> Result<bool> {
        Ok(self.inner.remove_key(key_id)?)
    }

    /// List all TACT keys with optional filters
    pub fn list_keys(
        &mut self,
        source_filter: Option<&str>,
        product_filter: Option<&str>,
        verified_only: bool,
    ) -> Vec<(cascette_crypto::TactKey, cascette_crypto::KeyMetadata)> {
        self.inner
            .list_keys(source_filter, product_filter, verified_only)
    }

    /// Get statistics about the key store
    pub fn get_stats(&mut self) -> TactKeyStats {
        self.inner.get_stats()
    }

    /// Import keys from a file
    #[allow(dead_code)] // May be used for future file import functionality
    pub fn import_from_file(&mut self, path: &Path, source: &str) -> Result<usize> {
        let content = std::fs::read_to_string(path)?;
        let mut imported = 0;

        for line in content.lines() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }

            // Parse format: LOOKUP_HASH ENCRYPTION_KEY [DESCRIPTION]
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let key_id_hex = parts[0].trim_start_matches("0x");
                let key_hex = parts[1];

                // Parse key ID
                if let Ok(key_id) = u64::from_str_radix(key_id_hex, 16) {
                    // Get optional description
                    let description = if parts.len() > 2 {
                        Some(parts[2..].join(" "))
                    } else {
                        None
                    };

                    // Try to add the key
                    if let Err(e) = self.add_key(key_id, key_hex, source, description, None, None) {
                        eprintln!("Warning: Failed to import key {:016X}: {}", key_id, e);
                        continue;
                    }

                    imported += 1;
                }
            }
        }

        Ok(imported)
    }

    /// Export keys to a file
    #[allow(dead_code)] // May be used for future export functionality
    pub fn export_to_file(&mut self, path: &Path, include_metadata: bool) -> Result<usize> {
        let mut content = String::new();

        // Add header
        content.push_str("# TACT Encryption Keys\n");
        writeln!(
            &mut content,
            "# Exported by cascette on {}",
            chrono::Utc::now()
        )
        .expect("Failed to write to string buffer");
        content.push_str("# Format: LOOKUP_HASH ENCRYPTION_KEY [DESCRIPTION]\n\n");

        let keys = self.list_keys(None, None, false);

        for (key, metadata) in &keys {
            let key_hex = ::hex::encode(key.key);
            if include_metadata {
                writeln!(
                    &mut content,
                    "{:016X} {} # {} ({})",
                    key.id,
                    key_hex,
                    metadata.description.as_deref().unwrap_or(""),
                    metadata.source
                )
                .expect("Failed to write to string buffer");
            } else {
                writeln!(&mut content, "{:016X} {}", key.id, key_hex)
                    .expect("Failed to write to string buffer");
            }
        }

        std::fs::write(path, content)?;
        Ok(keys.len())
    }

    /// Mark a key as verified
    pub fn mark_verified(&mut self, key_id: u64) -> Result<()> {
        match self.inner.get_key(key_id)? {
            Some((key, Some(mut metadata))) => {
                metadata.last_verified = Some(chrono::Utc::now());
                // Re-add the key with updated metadata
                self.inner.add_key(
                    key.id,
                    &::hex::encode(key.key),
                    &metadata.source,
                    metadata.description,
                    metadata.product,
                    metadata.build,
                )?;
                Ok(())
            }
            Some((_, None)) => Err(anyhow::anyhow!("Key {:016X} has no metadata", key_id)),
            None => Err(anyhow::anyhow!("Key {:016X} not found", key_id)),
        }
    }

    /// Save both keyring store and metadata to persistent storage (for batch operations)
    pub fn save_store(&mut self) -> Result<()> {
        self.inner.save_store()?;
        Ok(())
    }

    /// Check if a key exists (fast lookup without retrieval)
    #[allow(dead_code)]
    pub fn has_key(&mut self, key_id: u64) -> bool {
        self.inner.has_key(key_id)
    }
}
