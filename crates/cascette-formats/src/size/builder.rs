//! Builder for constructing Size manifests

use crate::install::TagType;
use crate::size::SizeTag;
use crate::size::entry::SizeEntry;
use crate::size::error::{Result, SizeError};
use crate::size::header::SizeHeader;
use crate::size::manifest::SizeManifest;

/// Builder for constructing `SizeManifest` instances
///
/// The builder collects entries and configuration, then computes
/// the derived header fields (total_size, entry_count) at build time.
pub struct SizeManifestBuilder {
    version: u8,
    key_size_bits: u8,
    tag_count: u16,
    esize_bytes: u8,
    total_size_override: Option<u64>,
    tags: Vec<SizeTag>,
    entries: Vec<SizeEntry>,
}

impl SizeManifestBuilder {
    /// Create a new builder with default settings
    ///
    /// Defaults: version 2, key_size_bits 72 (9-byte keys), tag_count 0, esize_bytes 4
    #[must_use]
    pub fn new() -> Self {
        Self {
            version: 2,
            key_size_bits: 72, // 72 bits = 9 bytes
            tag_count: 0,
            esize_bytes: 4,
            total_size_override: None,
            tags: Vec::new(),
            entries: Vec::new(),
        }
    }

    /// Set the format version (1 or 2)
    #[must_use]
    pub fn version(mut self, version: u8) -> Self {
        self.version = version;
        self
    }

    /// Set the key size in **bits** (e.g. 72 for 9-byte keys, 128 for 16-byte keys)
    ///
    /// The byte count used for on-disk storage is `(key_size_bits + 7) >> 3`.
    #[must_use]
    pub fn key_size_bits(mut self, bits: u8) -> Self {
        self.key_size_bits = bits;
        self
    }

    /// Set the tag count
    ///
    /// This sets the expected tag count in the header. Tags must be added
    /// to the manifest after building, or use `add_tag()`.
    #[must_use]
    pub fn tag_count(mut self, count: u16) -> Self {
        self.tag_count = count;
        self
    }

    /// Set the esize byte width (V1 only, ignored for V2)
    #[must_use]
    pub fn esize_bytes(mut self, width: u8) -> Self {
        self.esize_bytes = width;
        self
    }

    /// Set the total_size field explicitly
    ///
    /// When `esize_bytes` is 0, entries carry no per-entry size data and the
    /// total_size cannot be computed from entries. Use this method to set the
    /// aggregate size stored in the header.
    #[must_use]
    pub fn total_size(mut self, size: u64) -> Self {
        self.total_size_override = Some(size);
        self
    }

    /// Add a tag with the given name and type
    ///
    /// The tag's bit mask is sized to the current entry count at build time.
    #[must_use]
    pub fn add_tag(mut self, name: String, tag_type: TagType) -> Self {
        self.tags.push(SizeTag::new(name, tag_type, 0));
        self
    }

    /// Mark a file as associated with a tag
    ///
    /// # Panics
    ///
    /// Panics if `tag_index` is out of bounds.
    #[must_use]
    pub fn tag_file(mut self, tag_index: usize, file_index: usize) -> Self {
        // Ensure bit_mask is large enough
        let needed = (file_index + 1).div_ceil(8);
        if self.tags[tag_index].bit_mask.len() < needed {
            self.tags[tag_index].bit_mask.resize(needed, 0);
        }
        self.tags[tag_index].add_file(file_index);
        self
    }

    /// Add an entry with the given key, key hash, and estimated size
    ///
    /// `key_hash` must not be 0x0000 or 0xFFFF (reserved sentinel values).
    #[must_use]
    pub fn add_entry(mut self, key: Vec<u8>, key_hash: u16, esize: u64) -> Self {
        self.entries.push(SizeEntry::new(key, key_hash, esize));
        self
    }

    /// Build the final `SizeManifest`
    ///
    /// Computes total_size from the sum of entry esizes and entry_count
    /// from the number of added entries. If tags were added via `add_tag()`,
    /// tag_count is set automatically and bit masks are resized.
    pub fn build(mut self) -> Result<SizeManifest> {
        if self.version == 0 || self.version > 2 {
            return Err(SizeError::UnsupportedVersion(self.version));
        }

        // Validate key_size_bits produces a valid byte count (1-16)
        let key_bytes = (self.key_size_bits.saturating_add(7)) >> 3;
        if key_bytes == 0 || key_bytes > 16 {
            return Err(SizeError::InvalidEKeySize(self.key_size_bits));
        }

        // If tags were added via add_tag(), update tag_count
        if !self.tags.is_empty() {
            self.tag_count = self.tags.len() as u16;
        }

        let entry_count = self.entries.len() as u32;
        let total_size: u64 = self
            .total_size_override
            .unwrap_or_else(|| self.entries.iter().map(|e| e.esize).sum());

        // Resize tag bit masks to match entry count
        let bit_mask_size = (self.entries.len()).div_ceil(8);
        for tag in &mut self.tags {
            tag.bit_mask.resize(bit_mask_size, 0);
        }

        let header = match self.version {
            1 => {
                if self.esize_bytes > 8 {
                    return Err(SizeError::InvalidEsizeWidth(self.esize_bytes));
                }
                SizeHeader::new_v1(
                    self.key_size_bits,
                    entry_count,
                    self.tag_count,
                    total_size,
                    self.esize_bytes,
                )
            }
            2 => SizeHeader::new_v2(self.key_size_bits, entry_count, self.tag_count, total_size),
            _ => unreachable!(),
        };

        let manifest = SizeManifest {
            header,
            tags: self.tags,
            entries: self.entries,
        };

        // Validate the constructed manifest
        manifest.validate()?;

        Ok(manifest)
    }
}

impl Default for SizeManifestBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_defaults() {
        let manifest = SizeManifestBuilder::new()
            .add_entry(vec![0xAA; 9], 0x0001, 100)
            .build()
            .expect("Should build with defaults");

        assert_eq!(manifest.header.version(), 2);
        assert_eq!(manifest.header.key_size_bits(), 72);
        assert_eq!(manifest.header.ekey_size(), 9);
        assert_eq!(manifest.header.tag_count(), 0);
        assert_eq!(manifest.header.esize_bytes(), 4);
        assert_eq!(manifest.header.entry_count(), 1);
        assert_eq!(manifest.header.total_size(), 100);
        assert_eq!(manifest.tags.len(), 0);
    }

    #[test]
    fn test_builder_v1() {
        let manifest = SizeManifestBuilder::new()
            .version(1)
            .esize_bytes(2)
            .key_size_bits(72)
            .add_entry(vec![0x11; 9], 0x0001, 50)
            .add_entry(vec![0x22; 9], 0x0002, 75)
            .build()
            .expect("Should build V1 manifest");

        assert_eq!(manifest.header.version(), 1);
        assert_eq!(manifest.header.esize_bytes(), 2);
        assert_eq!(manifest.header.entry_count(), 2);
        assert_eq!(manifest.header.total_size(), 125);
    }

    #[test]
    fn test_builder_v2() {
        let manifest = SizeManifestBuilder::new()
            .version(2)
            .key_size_bits(72)
            .add_entry(vec![0xCC; 9], 0x0001, 1000)
            .build()
            .expect("Should build V2 manifest");

        assert_eq!(manifest.header.version(), 2);
        assert_eq!(manifest.header.key_size_bits(), 72);
        assert_eq!(manifest.header.ekey_size(), 9);
        assert_eq!(manifest.header.esize_bytes(), 4);
        assert_eq!(manifest.header.total_size(), 1000);
    }

    #[test]
    fn test_builder_empty_manifest() {
        let manifest = SizeManifestBuilder::new()
            .build()
            .expect("Should build empty manifest");

        assert_eq!(manifest.entries.len(), 0);
        assert_eq!(manifest.tags.len(), 0);
        assert_eq!(manifest.header.total_size(), 0);
    }

    #[test]
    fn test_builder_rejects_version_0() {
        let result = SizeManifestBuilder::new().version(0).build();
        assert!(matches!(result, Err(SizeError::UnsupportedVersion(0))));
    }

    #[test]
    fn test_builder_rejects_version_3() {
        let result = SizeManifestBuilder::new().version(3).build();
        assert!(matches!(result, Err(SizeError::UnsupportedVersion(3))));
    }

    #[test]
    fn test_builder_rejects_zero_key_size_bits() {
        // 0 bits → 0 bytes, invalid
        let result = SizeManifestBuilder::new().key_size_bits(0).build();
        assert!(matches!(result, Err(SizeError::InvalidEKeySize(0))));
    }

    #[test]
    fn test_builder_rejects_oversized_key_size_bits() {
        // 129 bits → 17 bytes, exceeds 16-byte maximum
        let result = SizeManifestBuilder::new().key_size_bits(129).build();
        assert!(matches!(result, Err(SizeError::InvalidEKeySize(129))));
    }

    #[test]
    fn test_builder_accepts_esize_bytes_0_v1() {
        let manifest = SizeManifestBuilder::new()
            .version(1)
            .esize_bytes(0)
            .key_size_bits(72)
            .add_entry(vec![0xAA; 9], 0x0001, 0)
            .build()
            .expect("Should build V1 manifest with esize_bytes=0");

        assert_eq!(manifest.header.version(), 1);
        assert_eq!(manifest.header.esize_bytes(), 0);
        assert_eq!(manifest.entries.len(), 1);
    }

    #[test]
    fn test_builder_rejects_invalid_esize_bytes_v1() {
        let result = SizeManifestBuilder::new().version(1).esize_bytes(9).build();
        assert!(matches!(result, Err(SizeError::InvalidEsizeWidth(9))));
    }

    #[test]
    fn test_builder_with_tags() {
        let manifest = SizeManifestBuilder::new()
            .version(2)
            .key_size_bits(72)
            .add_entry(vec![0xAA; 9], 0x0001, 100)
            .add_entry(vec![0xBB; 9], 0x0002, 200)
            .add_tag("Windows".to_string(), TagType::Platform)
            .tag_file(0, 0)
            .tag_file(0, 1)
            .add_tag("x86_64".to_string(), TagType::Architecture)
            .tag_file(1, 0)
            .build()
            .expect("Should build manifest with tags");

        assert_eq!(manifest.header.tag_count(), 2);
        assert_eq!(manifest.tags.len(), 2);
        assert_eq!(manifest.tags[0].name, "Windows");
        assert!(manifest.tags[0].has_file(0));
        assert!(manifest.tags[0].has_file(1));
        assert_eq!(manifest.tags[1].name, "x86_64");
        assert!(manifest.tags[1].has_file(0));
        assert!(!manifest.tags[1].has_file(1));
    }

    #[test]
    fn test_builder_tag_count_auto_set() {
        let manifest = SizeManifestBuilder::new()
            .add_tag("Test".to_string(), TagType::Platform)
            .build()
            .expect("Should build");

        assert_eq!(manifest.header.tag_count(), 1);
    }
}
