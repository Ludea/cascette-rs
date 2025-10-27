//! TACT key manager for centralized key orchestration
//!
//! This manager provides high-level orchestration for TACT key operations,
//! managing metadata and policies while delegating cryptographic operations
//! to cascette-crypto.

use crate::error::{MetadataError, MetadataResult};
use cascette_crypto::{FileBasedTactKeyStore, FileStoreConfig, KeyMetadata, TactKey};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

/// Statistics about the TACT key store
#[derive(Debug, Serialize, Clone, Default)]
pub struct TactKeyStats {
    /// Total number of keys
    pub total_keys: usize,
    /// Keys by source
    pub by_source: HashMap<String, usize>,
    /// Keys by product
    pub by_product: HashMap<String, usize>,
    /// Number of verified keys
    pub verified_keys: usize,
    /// Last update time
    pub last_update: Option<chrono::DateTime<chrono::Utc>>,
}

/// Manager for TACT encryption key orchestration
///
/// This manager handles the orchestration layer for TACT keys, managing
/// metadata, policies, and lifecycle while using cascette-crypto for
/// the actual storage and cryptographic operations.
pub struct TactKeyManager {
    /// The underlying file-based store (from cascette-crypto)
    store: FileBasedTactKeyStore,
}

impl TactKeyManager {
    /// Create a new TACT key manager with the given data directory
    pub fn new(data_dir: impl AsRef<Path>) -> MetadataResult<Self> {
        // Configure file-based storage
        let mut config = FileStoreConfig::production();
        config.keys_directory = data_dir.as_ref().join("tact_keys");

        let mut store = FileBasedTactKeyStore::with_config(config)?;

        // Ensure master password is initialized
        store.ensure_master_password()?;

        Ok(Self { store })
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
    ) -> MetadataResult<()> {
        // Validate hex format
        if key_hex.len() != 32 || !key_hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(MetadataError::InvalidKeyFormat(
                "Key must be 32 hex characters".to_string(),
            ));
        }

        // Create TactKey using cascette-crypto
        let tact_key = TactKey::from_hex(key_id, key_hex)?;

        // Create metadata
        let metadata = KeyMetadata {
            key_id,
            source: source.to_string(),
            description,
            added_at: chrono::Utc::now(),
            last_verified: None,
            product,
            build,
        };

        self.store.add(tact_key, metadata)?;
        Ok(())
    }

    /// Add a new TACT key for batch operations
    ///
    /// For file-based store, batch operations are the same as regular operations
    /// since each key is stored in its own file.
    pub fn add_key_batch(
        &mut self,
        key_id: u64,
        key_hex: &str,
        source: &str,
        description: Option<String>,
        product: Option<String>,
        build: Option<u32>,
    ) -> MetadataResult<()> {
        self.add_key(key_id, key_hex, source, description, product, build)
    }

    /// Add a new TACT key with verification status
    ///
    /// This is used when importing keys that are already verified by the community,
    /// such as keys from the WoWDev/TACTKeys repository.
    pub fn add_verified_key(
        &mut self,
        key_id: u64,
        key_hex: &str,
        source: &str,
        description: Option<String>,
        product: Option<String>,
        build: Option<u32>,
    ) -> MetadataResult<()> {
        // Parse the key
        let key_bytes =
            hex::decode(key_hex).map_err(|e| MetadataError::InvalidKeyFormat(e.to_string()))?;
        if key_bytes.len() != 16 {
            return Err(MetadataError::InvalidKeyFormat(
                "Key must be exactly 16 bytes".to_string(),
            ));
        }

        let mut key_array = [0u8; 16];
        key_array.copy_from_slice(&key_bytes);

        let tact_key = TactKey::new(key_id, key_array);

        // Create metadata with verification timestamp
        let now = chrono::Utc::now();
        let metadata = KeyMetadata {
            key_id,
            source: source.to_string(),
            description,
            added_at: now,
            last_verified: Some(now), // Mark as verified at import time
            product,
            build,
        };

        self.store.add(tact_key, metadata)?;
        Ok(())
    }

    /// Add a new verified TACT key for batch operations
    pub fn add_verified_key_batch(
        &mut self,
        key_id: u64,
        key_hex: &str,
        source: &str,
        description: Option<String>,
        product: Option<String>,
        build: Option<u32>,
    ) -> MetadataResult<()> {
        self.add_verified_key(key_id, key_hex, source, description, product, build)
    }

    /// Get a TACT key by ID
    pub fn get_key(
        &mut self,
        key_id: u64,
    ) -> MetadataResult<Option<(TactKey, Option<KeyMetadata>)>> {
        match self.store.get(key_id) {
            Ok(Some((key, metadata))) => Ok(Some((key, Some(metadata)))),
            Ok(None) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Check if a key exists (fast metadata-only check)
    pub fn has_key(&mut self, key_id: u64) -> bool {
        // Try to get just metadata for faster lookup
        matches!(self.store.get(key_id), Ok(Some(_)))
    }

    /// Remove a TACT key
    pub fn remove_key(&mut self, key_id: u64) -> MetadataResult<bool> {
        Ok(self.store.remove(key_id)?)
    }

    /// List all TACT keys with optional filters
    pub fn list_keys(
        &mut self,
        source_filter: Option<&str>,
        product_filter: Option<&str>,
        verified_only: bool,
    ) -> Vec<(TactKey, KeyMetadata)> {
        let Ok(mut results) = self.store.list_keys() else {
            return Vec::new();
        };

        // Apply filters
        results.retain(|(_, metadata)| {
            if let Some(source) = source_filter {
                if !metadata.source.contains(source) {
                    return false;
                }
            }

            if let Some(product) = product_filter {
                if metadata.product.as_deref() != Some(product) {
                    return false;
                }
            }

            if verified_only && metadata.last_verified.is_none() {
                return false;
            }

            true
        });

        // Sort by key ID for consistent output
        results.sort_by_key(|(k, _)| k.id);

        results
    }

    /// Get statistics about the key store
    pub fn get_stats(&mut self) -> TactKeyStats {
        let Ok(keys) = self.store.list_keys() else {
            return TactKeyStats {
                total_keys: 0,
                by_source: HashMap::new(),
                by_product: HashMap::new(),
                verified_keys: 0,
                last_update: None,
            };
        };

        let total_keys = keys.len();
        let mut by_source = HashMap::new();
        let mut by_product = HashMap::new();
        let mut verified_keys = 0;
        let mut last_update = None;

        for (_, metadata) in &keys {
            *by_source.entry(metadata.source.clone()).or_insert(0) += 1;

            if let Some(product) = &metadata.product {
                *by_product.entry(product.clone()).or_insert(0) += 1;
            }

            if metadata.last_verified.is_some() {
                verified_keys += 1;
            }

            match last_update {
                None => last_update = Some(metadata.added_at),
                Some(last) if metadata.added_at > last => last_update = Some(metadata.added_at),
                _ => {}
            }
        }

        TactKeyStats {
            total_keys,
            by_source,
            by_product,
            verified_keys,
            last_update,
        }
    }

    /// Save the key store to disk
    ///
    /// This ensures all keys and metadata are persisted.
    pub fn save_store(&mut self) -> MetadataResult<()> {
        // For file-based store, saving is automatic with each operation
        // but we can force a metadata sync if needed
        Ok(())
    }

    /// Clear all keys from the store
    pub fn clear_all(&mut self) -> MetadataResult<()> {
        // FileBasedTactKeyStore doesn't have a clear method
        // We need to remove keys individually
        let keys = self.store.list_keys()?;
        for (key, _) in keys {
            self.store.remove(key.id)?;
        }
        Ok(())
    }

    /// Export all keys for backup
    pub fn export_keys(&mut self) -> MetadataResult<Vec<(u64, String, KeyMetadata)>> {
        let keys = self.store.list_keys()?;
        let mut exports = Vec::new();

        for (key, metadata) in keys {
            let key_hex = hex::encode(key.key);
            exports.push((key.id, key_hex, metadata));
        }

        Ok(exports)
    }

    /// Import keys from backup
    pub fn import_keys(&mut self, keys: Vec<(u64, String, KeyMetadata)>) -> MetadataResult<()> {
        for (key_id, key_hex, metadata) in keys {
            let key_bytes = hex::decode(&key_hex)
                .map_err(|e| MetadataError::InvalidKeyFormat(e.to_string()))?;
            if key_bytes.len() != 16 {
                continue; // Skip invalid keys
            }

            let mut key_array = [0u8; 16];
            key_array.copy_from_slice(&key_bytes);

            let tact_key = TactKey::new(key_id, key_array);
            self.store.add(tact_key, metadata)?;
        }

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_key_manager_creation() {
        let temp_dir = TempDir::new().expect("Test assertion");
        let manager = TactKeyManager::new(temp_dir.path());
        assert!(manager.is_ok());
    }

    #[test]
    fn test_add_and_get_key() {
        let temp_dir = TempDir::new().expect("Test assertion");
        let mut manager = TactKeyManager::new(temp_dir.path()).expect("Test assertion");

        // Add a key
        let key_id = 0x1234_5678_90AB_CDEF;
        let key_hex = "0123456789ABCDEF0123456789ABCDEF";

        manager
            .add_key(
                key_id,
                key_hex,
                "test",
                Some("Test key".to_string()),
                Some("wow".to_string()),
                Some(12345),
            )
            .expect("Test assertion");

        // Get the key back
        let result = manager.get_key(key_id).expect("Test assertion");
        assert!(result.is_some());

        let (key, metadata) = result.expect("Test assertion");
        assert_eq!(key.id, key_id);
        assert_eq!(metadata.as_ref().expect("Test assertion").source, "test");
    }

    #[test]
    fn test_verified_key() {
        let temp_dir = TempDir::new().expect("Test assertion");
        let mut manager = TactKeyManager::new(temp_dir.path()).expect("Test assertion");

        let key_id = 0xFEDC_BA09_8765_4321;
        let key_hex = "FEDCBA9876543210FEDCBA9876543210";

        manager
            .add_verified_key(
                key_id,
                key_hex,
                "wowdev",
                Some("Verified key".to_string()),
                None,
                None,
            )
            .expect("Test assertion");

        let result = manager.get_key(key_id).expect("Test assertion");
        assert!(result.is_some());

        let (_key, metadata) = result.expect("Test assertion");
        assert!(
            metadata
                .as_ref()
                .expect("Test assertion")
                .last_verified
                .is_some()
        );
    }

    #[test]
    fn test_list_keys_with_filters() {
        let temp_dir = TempDir::new().expect("Test assertion");
        let mut manager = TactKeyManager::new(temp_dir.path()).expect("Test assertion");

        // Add multiple keys
        manager
            .add_key(
                0x1111,
                "11111111111111111111111111111111",
                "source1",
                None,
                Some("wow".to_string()),
                None,
            )
            .expect("Test assertion");
        manager
            .add_verified_key(
                0x2222,
                "22222222222222222222222222222222",
                "source2",
                None,
                Some("wow".to_string()),
                None,
            )
            .expect("Test assertion");
        manager
            .add_key(
                0x3333,
                "33333333333333333333333333333333",
                "source1",
                None,
                Some("d4".to_string()),
                None,
            )
            .expect("Test assertion");

        // Test filters
        let all_keys = manager.list_keys(None, None, false);
        assert_eq!(all_keys.len(), 3);

        let source1_keys = manager.list_keys(Some("source1"), None, false);
        assert_eq!(source1_keys.len(), 2);

        let wow_keys = manager.list_keys(None, Some("wow"), false);
        assert_eq!(wow_keys.len(), 2);

        let verified_keys = manager.list_keys(None, None, true);
        assert_eq!(verified_keys.len(), 1);
    }
}
