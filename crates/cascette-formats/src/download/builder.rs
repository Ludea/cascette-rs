//! Builder for download manifests with version support

use crate::download::entry::{DownloadFileEntry, FileSize40};
use crate::download::error::{DownloadError, Result};
use crate::download::header::DownloadHeader;
use crate::download::manifest::DownloadManifest;
use crate::download::tag::DownloadTag;
use crate::install::TagType;
use cascette_crypto::EncodingKey;
use std::collections::HashMap;

/// Builder for creating download manifests with version-specific features
///
/// The builder enforces version compatibility and provides a fluent API
/// for constructing valid download manifests across all supported versions.
///
/// # Version Support
///
/// - **V1**: Basic entries with encoding keys and priorities
/// - **V2**: Adds flag support for entries
/// - **V3**: Adds base priority adjustment and reserved fields
///
/// # Examples
///
/// ```rust,no_run
/// use cascette_formats::download::{DownloadManifestBuilder, TagType};
/// use cascette_crypto::EncodingKey;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let ekey = EncodingKey::from_hex("0123456789abcdef0123456789abcdef")?;
///
/// let manifest = DownloadManifestBuilder::new(3)? // Version 3
///     .with_checksums(true)
///     .with_flags(2)?
///     .with_base_priority(-1)?
///     .add_file(ekey, 1024, 0)? // Essential file
///     .add_tag("Windows".to_string(), TagType::Platform)
///     .associate_file_with_tag(0, "Windows")?
///     .set_file_checksum(0, 0x1234_5678)?
///     .set_file_flags(0, vec![0xAB, 0xCD])?
///     .build()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct DownloadManifestBuilder {
    /// Format version (1, 2, or 3)
    version: u8,
    /// File entries in order
    entries: Vec<DownloadFileEntry>,
    /// Tags for file categorization
    tags: Vec<DownloadTag>,
    /// Whether entries have checksums
    has_checksum: bool,
    /// Flag size for V2+ (0 = no flags)
    flag_size: u8,
    /// Base priority adjustment for V3+ (0 for V1/V2)
    base_priority: i8,
    /// Map of tag names to indices for quick lookup
    tag_name_to_index: HashMap<String, usize>,
}

impl DownloadManifestBuilder {
    /// Create a new builder for the specified version
    pub fn new(version: u8) -> Result<Self> {
        if !(1..=3).contains(&version) {
            return Err(DownloadError::UnsupportedVersion(version));
        }

        Ok(Self {
            version,
            entries: Vec::new(),
            tags: Vec::new(),
            has_checksum: false,
            flag_size: 0,
            base_priority: 0,
            tag_name_to_index: HashMap::new(),
        })
    }

    /// Enable or disable checksums for entries
    #[must_use]
    pub fn with_checksums(mut self, enable: bool) -> Self {
        self.has_checksum = enable;
        self
    }

    /// Set flag size for entries (V2+ only)
    ///
    /// Returns an error if flags are requested on V1 manifests.
    pub fn with_flags(mut self, flag_size: u8) -> Result<Self> {
        if self.version < 2 && flag_size > 0 {
            return Err(DownloadError::FlagsNotSupportedInVersion(self.version));
        }
        self.flag_size = flag_size;
        Ok(self)
    }

    /// Set base priority adjustment (V3+ only)
    ///
    /// Returns an error if base priority is set on V1/V2 manifests.
    pub fn with_base_priority(mut self, base_priority: i8) -> Result<Self> {
        if self.version < 3 && base_priority != 0 {
            return Err(DownloadError::BasePriorityNotSupportedInVersion(
                self.version,
            ));
        }
        self.base_priority = base_priority;
        Ok(self)
    }

    /// Add a file entry to the manifest
    ///
    /// Files are added in order and will appear in that order in the final manifest.
    /// The file index can be used with other methods to set additional properties.
    pub fn add_file(
        mut self,
        encoding_key: EncodingKey,
        file_size: u64,
        priority: i8,
    ) -> Result<Self> {
        let size = FileSize40::new(file_size)?;

        let entry = DownloadFileEntry {
            encoding_key,
            file_size: size,
            priority,
            checksum: None,
            flags: if self.flag_size > 0 {
                Some(vec![0; self.flag_size as usize])
            } else {
                None
            },
        };

        self.entries.push(entry);

        // Resize tag bit masks to accommodate new entry
        let new_mask_size = self.entries.len().div_ceil(8);
        for tag in &mut self.tags {
            if tag.bit_mask.len() < new_mask_size {
                tag.bit_mask.resize(new_mask_size, 0);
            }
        }

        Ok(self)
    }

    /// Add a tag to the manifest
    ///
    /// Tags are used to categorize files for selective downloading.
    /// The tag name must be unique within the manifest.
    #[must_use]
    pub fn add_tag(mut self, name: String, tag_type: TagType) -> Self {
        let bit_mask_size = self.entries.len().div_ceil(8);
        let tag = DownloadTag {
            name: name.clone(),
            tag_type,
            bit_mask: vec![0u8; bit_mask_size],
        };

        let tag_index = self.tags.len();
        self.tag_name_to_index.insert(name, tag_index);
        self.tags.push(tag);

        self
    }

    /// Set checksum for a specific file
    ///
    /// Returns an error if:
    /// - Checksums are not enabled
    /// - File index is out of bounds
    pub fn set_file_checksum(mut self, file_index: usize, checksum: u32) -> Result<Self> {
        if !self.has_checksum {
            return Err(DownloadError::ChecksumsNotEnabled);
        }

        let entry = self
            .entries
            .get_mut(file_index)
            .ok_or(DownloadError::FileIndexOutOfBounds(file_index))?;
        entry.checksum = Some(checksum);
        Ok(self)
    }

    /// Set flags for a specific file
    ///
    /// Returns an error if:
    /// - Flags are not enabled (`flag_size` = 0)
    /// - Flag data length doesn't match `flag_size`
    /// - File index is out of bounds
    pub fn set_file_flags(mut self, file_index: usize, flags: Vec<u8>) -> Result<Self> {
        if self.flag_size == 0 {
            return Err(DownloadError::FlagsNotEnabled);
        }

        if flags.len() != self.flag_size as usize {
            return Err(DownloadError::InvalidFlagSize(flags.len(), self.flag_size));
        }

        let entry = self
            .entries
            .get_mut(file_index)
            .ok_or(DownloadError::FileIndexOutOfBounds(file_index))?;
        entry.flags = Some(flags);
        Ok(self)
    }

    /// Associate a file with a tag
    ///
    /// Returns an error if:
    /// - File index is out of bounds
    /// - Tag name is not found
    pub fn associate_file_with_tag(mut self, file_index: usize, tag_name: &str) -> Result<Self> {
        if file_index >= self.entries.len() {
            return Err(DownloadError::FileIndexOutOfBounds(file_index));
        }

        let tag_index = *self
            .tag_name_to_index
            .get(tag_name)
            .ok_or_else(|| DownloadError::TagNotFound(tag_name.to_string()))?;

        self.tags[tag_index].add_file(file_index);
        Ok(self)
    }

    /// Remove file association from a tag
    pub fn disassociate_file_from_tag(mut self, file_index: usize, tag_name: &str) -> Result<Self> {
        if file_index >= self.entries.len() {
            return Err(DownloadError::FileIndexOutOfBounds(file_index));
        }

        let tag_index = *self
            .tag_name_to_index
            .get(tag_name)
            .ok_or_else(|| DownloadError::TagNotFound(tag_name.to_string()))?;

        self.tags[tag_index].remove_file(file_index);
        Ok(self)
    }

    /// Set multiple properties for a file at once
    ///
    /// This is a convenience method that combines setting checksum and flags.
    pub fn configure_file(
        mut self,
        file_index: usize,
        checksum: Option<u32>,
        flags: Option<Vec<u8>>,
    ) -> Result<Self> {
        if let Some(checksum) = checksum {
            self = self.set_file_checksum(file_index, checksum)?;
        }

        if let Some(flags) = flags {
            self = self.set_file_flags(file_index, flags)?;
        }

        Ok(self)
    }

    /// Associate a file with multiple tags at once
    pub fn associate_file_with_tags(
        mut self,
        file_index: usize,
        tag_names: &[&str],
    ) -> Result<Self> {
        for tag_name in tag_names {
            self = self.associate_file_with_tag(file_index, tag_name)?;
        }
        Ok(self)
    }

    /// Add a file with all properties set at once
    ///
    /// This is a convenience method that combines `add_file` with property setting.
    pub fn add_file_with_properties(
        mut self,
        encoding_key: EncodingKey,
        file_size: u64,
        priority: i8,
        checksum: Option<u32>,
        flags: Option<Vec<u8>>,
        tag_names: Option<&[&str]>,
    ) -> Result<Self> {
        let file_index = self.entries.len();

        self = self.add_file(encoding_key, file_size, priority)?;

        if let Some(checksum) = checksum {
            self = self.set_file_checksum(file_index, checksum)?;
        }

        if let Some(flags) = flags {
            self = self.set_file_flags(file_index, flags)?;
        }

        if let Some(tag_names) = tag_names {
            self = self.associate_file_with_tags(file_index, tag_names)?;
        }

        Ok(self)
    }

    /// Get current number of entries
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Get current number of tags
    pub fn tag_count(&self) -> usize {
        self.tags.len()
    }

    /// Check if a tag exists
    pub fn has_tag(&self, tag_name: &str) -> bool {
        self.tag_name_to_index.contains_key(tag_name)
    }

    /// Get list of all tag names
    pub fn tag_names(&self) -> Vec<&str> {
        self.tag_name_to_index
            .keys()
            .map(std::string::String::as_str)
            .collect()
    }

    /// Get current builder configuration summary
    pub fn config_summary(&self) -> BuilderConfig {
        BuilderConfig {
            version: self.version,
            entry_count: self.entries.len(),
            tag_count: self.tags.len(),
            has_checksum: self.has_checksum,
            flag_size: self.flag_size,
            base_priority: self.base_priority,
        }
    }

    /// Validate the current state without building
    pub fn validate(&self) -> Result<()> {
        // Validate version-specific constraints
        if self.version < 2 && self.flag_size > 0 {
            return Err(DownloadError::FlagsNotSupportedInVersion(self.version));
        }

        if self.version < 3 && self.base_priority != 0 {
            return Err(DownloadError::BasePriorityNotSupportedInVersion(
                self.version,
            ));
        }

        // Validate entries
        for entry in &self.entries {
            // Check checksum consistency
            if self.has_checksum && entry.checksum.is_none() {
                return Err(DownloadError::MissingChecksum);
            }
            if !self.has_checksum && entry.checksum.is_some() {
                return Err(DownloadError::ChecksumsNotEnabled);
            }

            // Check flags consistency
            if self.flag_size > 0 && entry.flags.is_none() {
                return Err(DownloadError::MissingFlags);
            }
            if self.flag_size == 0 && entry.flags.is_some() {
                return Err(DownloadError::FlagsNotEnabled);
            }
            if let Some(ref flags) = entry.flags {
                if flags.len() != self.flag_size as usize {
                    return Err(DownloadError::InvalidFlagSize(flags.len(), self.flag_size));
                }
            }
        }

        // Validate tags
        let expected_mask_size = self.entries.len().div_ceil(8);
        for tag in &self.tags {
            if tag.bit_mask.len() != expected_mask_size {
                return Err(DownloadError::BitMaskSizeMismatch);
            }
        }

        Ok(())
    }

    /// Build the final download manifest
    ///
    /// This validates the builder state and creates the manifest with the
    /// appropriate version-specific header.
    pub fn build(self) -> Result<DownloadManifest> {
        self.validate()?;

        let header = match self.version {
            1 => DownloadHeader::new_v1(
                self.entries.len() as u32,
                self.tags.len() as u16,
                self.has_checksum,
            ),
            2 => DownloadHeader::new_v2(
                self.entries.len() as u32,
                self.tags.len() as u16,
                self.has_checksum,
                self.flag_size,
            ),
            3 => DownloadHeader::new_v3(
                self.entries.len() as u32,
                self.tags.len() as u16,
                self.has_checksum,
                self.flag_size,
                self.base_priority,
            ),
            _ => return Err(DownloadError::UnsupportedVersion(self.version)),
        };

        let manifest = DownloadManifest {
            header,
            entries: self.entries,
            tags: self.tags,
        };

        // Final validation
        manifest.validate()?;

        Ok(manifest)
    }

    /// Create a builder from an existing manifest (for modification)
    pub fn from_manifest(manifest: &DownloadManifest) -> Self {
        let mut tag_name_to_index = HashMap::new();
        for (index, tag) in manifest.tags.iter().enumerate() {
            tag_name_to_index.insert(tag.name.clone(), index);
        }

        Self {
            version: manifest.header.version(),
            entries: manifest.entries.clone(),
            tags: manifest.tags.clone(),
            has_checksum: manifest.header.has_checksum(),
            flag_size: manifest.header.flag_size(),
            base_priority: manifest.header.base_priority(),
            tag_name_to_index,
        }
    }

    /// Clone the builder for creating variations
    #[must_use]
    pub fn clone_builder(&self) -> Self {
        self.clone()
    }
}

/// Configuration summary for a builder
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuilderConfig {
    /// Format version
    pub version: u8,
    /// Number of entries
    pub entry_count: usize,
    /// Number of tags
    pub tag_count: usize,
    /// Whether entries have checksums
    pub has_checksum: bool,
    /// Flag size (0 = no flags)
    pub flag_size: u8,
    /// Base priority adjustment
    pub base_priority: i8,
}

impl BuilderConfig {
    /// Check if configuration is valid for the specified version
    pub fn is_valid_for_version(&self, version: u8) -> bool {
        match version {
            1 => self.flag_size == 0 && self.base_priority == 0,
            2 => self.base_priority == 0,
            3 => true,
            _ => false,
        }
    }

    /// Get minimum version required for this configuration
    pub fn minimum_version_required(&self) -> u8 {
        if self.base_priority != 0 {
            3
        } else if self.flag_size > 0 {
            2
        } else {
            1
        }
    }
}

/// Preset builders for common scenarios
impl DownloadManifestBuilder {
    /// Create a basic V1 manifest builder
    pub fn basic() -> Result<Self> {
        Self::new(1)
    }

    /// Create a V2 manifest builder with flags
    pub fn with_flags_support(flag_size: u8) -> Result<Self> {
        Self::new(2)?.with_flags(flag_size)
    }

    /// Create a V3 manifest builder with all features
    pub fn full_featured(flag_size: u8, base_priority: i8) -> Result<Self> {
        Self::new(3)?
            .with_flags(flag_size)?
            .with_base_priority(base_priority)
    }

    /// Create a manifest for essential content only
    pub fn essential_content() -> Result<Self> {
        // V3 with negative base priority to emphasize essential content
        Self::full_featured(0, -10)
    }

    /// Create a manifest optimized for streaming
    pub fn streaming_optimized() -> Result<Self> {
        // V3 with base priority adjustment for fine-grained streaming control
        Self::full_featured(1, -5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::TagType;

    fn create_test_encoding_key() -> EncodingKey {
        EncodingKey::from_hex("0123456789abcdef0123456789abcdef").expect("Operation should succeed")
    }

    #[test]
    fn test_builder_creation() {
        // Valid versions
        assert!(DownloadManifestBuilder::new(1).is_ok());
        assert!(DownloadManifestBuilder::new(2).is_ok());
        assert!(DownloadManifestBuilder::new(3).is_ok());

        // Invalid versions
        assert!(DownloadManifestBuilder::new(0).is_err());
        assert!(DownloadManifestBuilder::new(4).is_err());
    }

    #[test]
    fn test_version_constraints() {
        // V1 - no flags or base priority
        assert!(
            DownloadManifestBuilder::new(1)
                .expect("Operation should succeed")
                .with_flags(1)
                .is_err()
        );
        assert!(
            DownloadManifestBuilder::new(1)
                .expect("Operation should succeed")
                .with_base_priority(-1)
                .is_err()
        );

        // V2 - flags ok, no base priority
        let builder = DownloadManifestBuilder::new(2).expect("Operation should succeed");
        assert!(builder.with_flags(1).is_ok());
        assert!(
            DownloadManifestBuilder::new(2)
                .expect("Operation should succeed")
                .with_base_priority(-1)
                .is_err()
        );

        // V3 - all features ok
        assert!(
            DownloadManifestBuilder::new(3)
                .expect("Operation should succeed")
                .with_flags(1)
                .is_ok()
        );
        assert!(
            DownloadManifestBuilder::new(3)
                .expect("Operation should succeed")
                .with_base_priority(-1)
                .is_ok()
        );
    }

    #[test]
    fn test_file_operations() {
        let ekey = create_test_encoding_key();

        let builder = DownloadManifestBuilder::new(2)
            .expect("Operation should succeed")
            .with_checksums(true)
            .with_flags(2)
            .expect("Operation should succeed")
            .add_file(ekey, 1024, 5)
            .expect("Operation should succeed")
            .add_file(ekey, 2048, 10)
            .expect("Operation should succeed");

        assert_eq!(builder.entry_count(), 2);

        // Test property setting
        let builder = builder
            .set_file_checksum(0, 0x1234_5678)
            .expect("Operation should succeed")
            .set_file_flags(0, vec![0xAB, 0xCD])
            .expect("Operation should succeed")
            .set_file_checksum(1, 0x8765_4321)
            .expect("Operation should succeed")
            .set_file_flags(1, vec![0xEF, 0x01])
            .expect("Operation should succeed");

        let manifest = builder.build().expect("Operation should succeed");
        assert_eq!(manifest.entries[0].checksum, Some(0x1234_5678));
        assert_eq!(
            manifest.entries[0]
                .flags
                .as_ref()
                .expect("Operation should succeed"),
            &vec![0xAB, 0xCD]
        );
        assert_eq!(manifest.entries[1].checksum, Some(0x8765_4321));
        assert_eq!(
            manifest.entries[1]
                .flags
                .as_ref()
                .expect("Operation should succeed"),
            &vec![0xEF, 0x01]
        );
    }

    #[test]
    fn test_tag_operations() {
        let ekey = create_test_encoding_key();

        let builder = DownloadManifestBuilder::new(1)
            .expect("Operation should succeed")
            .add_file(ekey, 1024, 0)
            .expect("Operation should succeed")
            .add_file(ekey, 2048, 5)
            .expect("Operation should succeed")
            .add_tag("Windows".to_string(), TagType::Platform)
            .add_tag("Optional".to_string(), TagType::Option);

        assert_eq!(builder.tag_count(), 2);
        assert!(builder.has_tag("Windows"));
        assert!(builder.has_tag("Optional"));
        assert!(!builder.has_tag("NonExistent"));

        let tag_names = builder.tag_names();
        assert!(tag_names.contains(&"Windows"));
        assert!(tag_names.contains(&"Optional"));

        let builder = builder
            .associate_file_with_tag(0, "Windows")
            .expect("Operation should succeed")
            .associate_file_with_tag(1, "Optional")
            .expect("Operation should succeed");

        let manifest = builder.build().expect("Operation should succeed");

        let windows_tag = manifest
            .find_tag("Windows")
            .expect("Operation should succeed");
        assert!(windows_tag.has_file(0));
        assert!(!windows_tag.has_file(1));

        let optional_tag = manifest
            .find_tag("Optional")
            .expect("Operation should succeed");
        assert!(!optional_tag.has_file(0));
        assert!(optional_tag.has_file(1));
    }

    #[test]
    fn test_validation_errors() {
        let ekey = create_test_encoding_key();

        // Missing checksum
        let builder = DownloadManifestBuilder::new(1)
            .expect("Operation should succeed")
            .with_checksums(true)
            .add_file(ekey, 1024, 0)
            .expect("Operation should succeed");

        assert!(matches!(
            builder.validate(),
            Err(DownloadError::MissingChecksum)
        ));

        // Missing flags - need to add file first, then try to validate without setting flags
        let _builder = DownloadManifestBuilder::new(2)
            .expect("Operation should succeed")
            .with_flags(1)
            .expect("Operation should succeed")
            .add_file(ekey, 1024, 0)
            .expect("Operation should succeed");

        // The add_file method automatically adds default flag bytes if flag_size > 0
        // So we need to manually create an entry without flags to test this
        let mut builder_manual = DownloadManifestBuilder::new(2).expect("Operation should succeed");
        builder_manual.flag_size = 1;
        builder_manual
            .entries
            .push(DownloadFileEntry::new(ekey, 1024, 0).expect("Operation should succeed"));

        assert!(matches!(
            builder_manual.validate(),
            Err(DownloadError::MissingFlags)
        ));

        // Wrong flag size - this should error in set_file_flags
        let result = DownloadManifestBuilder::new(2)
            .expect("Operation should succeed")
            .with_flags(2)
            .expect("Operation should succeed")
            .add_file(ekey, 1024, 0)
            .expect("Operation should succeed")
            .set_file_flags(0, vec![0xAB]); // 1 byte instead of 2

        assert!(matches!(result, Err(DownloadError::InvalidFlagSize(1, 2))));
    }

    #[test]
    fn test_convenience_methods() {
        let ekey = create_test_encoding_key();

        // Test configure_file
        let builder = DownloadManifestBuilder::new(2)
            .expect("Operation should succeed")
            .with_checksums(true)
            .with_flags(1)
            .expect("Operation should succeed")
            .add_file(ekey, 1024, 0)
            .expect("Operation should succeed")
            .configure_file(0, Some(0x1234_5678), Some(vec![0xAB]))
            .expect("Operation should succeed");

        let manifest = builder.build().expect("Operation should succeed");
        assert_eq!(manifest.entries[0].checksum, Some(0x1234_5678));
        assert_eq!(
            manifest.entries[0]
                .flags
                .as_ref()
                .expect("Operation should succeed"),
            &vec![0xAB]
        );

        // Test associate_file_with_tags
        let builder = DownloadManifestBuilder::new(1)
            .expect("Operation should succeed")
            .add_file(ekey, 1024, 0)
            .expect("Operation should succeed")
            .add_tag("Windows".to_string(), TagType::Platform)
            .add_tag("x86_64".to_string(), TagType::Architecture)
            .associate_file_with_tags(0, &["Windows", "x86_64"])
            .expect("Operation should succeed");

        let manifest = builder.build().expect("Operation should succeed");
        assert!(
            manifest
                .find_tag("Windows")
                .expect("Operation should succeed")
                .has_file(0)
        );
        assert!(
            manifest
                .find_tag("x86_64")
                .expect("Operation should succeed")
                .has_file(0)
        );

        // Test add_file_with_properties
        let builder = DownloadManifestBuilder::new(2)
            .expect("Operation should succeed")
            .with_checksums(true)
            .with_flags(1)
            .expect("Operation should succeed")
            .add_tag("Windows".to_string(), TagType::Platform)
            .add_file_with_properties(
                ekey,
                1024,
                0,
                Some(0x1234_5678),
                Some(vec![0xAB]),
                Some(&["Windows"]),
            )
            .expect("Operation should succeed");

        let manifest = builder.build().expect("Operation should succeed");
        assert_eq!(manifest.entries[0].checksum, Some(0x1234_5678));
        assert_eq!(
            manifest.entries[0]
                .flags
                .as_ref()
                .expect("Operation should succeed"),
            &vec![0xAB]
        );
        assert!(
            manifest
                .find_tag("Windows")
                .expect("Operation should succeed")
                .has_file(0)
        );
    }

    #[test]
    fn test_builder_config() {
        let builder = DownloadManifestBuilder::new(3)
            .expect("Operation should succeed")
            .with_checksums(true)
            .with_flags(2)
            .expect("Operation should succeed")
            .with_base_priority(-1)
            .expect("Operation should succeed");

        let config = builder.config_summary();
        assert_eq!(config.version, 3);
        assert!(config.has_checksum);
        assert_eq!(config.flag_size, 2);
        assert_eq!(config.base_priority, -1);
        assert_eq!(config.minimum_version_required(), 3);

        assert!(!config.is_valid_for_version(1));
        assert!(!config.is_valid_for_version(2));
        assert!(config.is_valid_for_version(3));
    }

    #[test]
    fn test_preset_builders() {
        // Basic V1
        let basic = DownloadManifestBuilder::basic().expect("Operation should succeed");
        assert_eq!(basic.version, 1);

        // V2 with flags
        let with_flags =
            DownloadManifestBuilder::with_flags_support(2).expect("Operation should succeed");
        assert_eq!(with_flags.version, 2);
        assert_eq!(with_flags.flag_size, 2);

        // V3 full featured
        let full = DownloadManifestBuilder::full_featured(1, -5).expect("Operation should succeed");
        assert_eq!(full.version, 3);
        assert_eq!(full.flag_size, 1);
        assert_eq!(full.base_priority, -5);

        // Essential content
        let essential =
            DownloadManifestBuilder::essential_content().expect("Operation should succeed");
        assert_eq!(essential.version, 3);
        assert_eq!(essential.base_priority, -10);

        // Streaming optimized
        let streaming =
            DownloadManifestBuilder::streaming_optimized().expect("Operation should succeed");
        assert_eq!(streaming.version, 3);
        assert_eq!(streaming.flag_size, 1);
        assert_eq!(streaming.base_priority, -5);
    }

    #[test]
    fn test_from_manifest() {
        let ekey = create_test_encoding_key();

        // Create original manifest
        let original = DownloadManifestBuilder::new(2)
            .expect("Operation should succeed")
            .with_checksums(true)
            .with_flags(1)
            .expect("Operation should succeed")
            .add_file(ekey, 1024, 0)
            .expect("Operation should succeed")
            .add_tag("Windows".to_string(), TagType::Platform)
            .associate_file_with_tag(0, "Windows")
            .expect("Operation should succeed")
            .set_file_checksum(0, 0x1234_5678)
            .expect("Operation should succeed")
            .set_file_flags(0, vec![0xAB])
            .expect("Operation should succeed")
            .build()
            .expect("Operation should succeed");

        // Create builder from manifest
        let builder = DownloadManifestBuilder::from_manifest(&original);
        assert_eq!(builder.version, 2);
        assert_eq!(builder.entry_count(), 1);
        assert_eq!(builder.tag_count(), 1);
        assert!(builder.has_tag("Windows"));

        // Build should produce identical manifest
        let rebuilt = builder.build().expect("Operation should succeed");
        assert_eq!(original, rebuilt);
    }

    #[test]
    fn test_builder_cloning() {
        let ekey = create_test_encoding_key();

        let base_builder = DownloadManifestBuilder::new(2)
            .expect("Operation should succeed")
            .add_file(ekey, 1024, 0)
            .expect("Operation should succeed");

        let builder1 = base_builder
            .clone_builder()
            .add_tag("Windows".to_string(), TagType::Platform)
            .associate_file_with_tag(0, "Windows")
            .expect("Operation should succeed");

        let builder2 = base_builder
            .clone_builder()
            .add_tag("Mac".to_string(), TagType::Platform)
            .associate_file_with_tag(0, "Mac")
            .expect("Operation should succeed");

        let manifest1 = builder1.build().expect("Operation should succeed");
        let manifest2 = builder2.build().expect("Operation should succeed");

        // Should have different tags but same entry structure
        assert_ne!(manifest1, manifest2);
        assert!(manifest1.find_tag("Windows").is_some());
        assert!(manifest2.find_tag("Mac").is_some());
        assert_eq!(manifest1.entries.len(), manifest2.entries.len());
    }
}
