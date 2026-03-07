//! Size manifest format (`DS` magic) for estimated file sizes
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::use_self)]
//!
//! The Size manifest maps encoding keys to estimated file sizes (eSize). It is
//! used when compressed size (cSize) is unavailable, enabling disk space
//! estimation and download progress reporting.
//!
//! # Format Overview
//!
//! - Magic: `DS` (0x44, 0x53)
//! - Versions: 1 (variable esize width) and 2 (fixed 4-byte esize)
//! - All multi-byte integers: big-endian
//! - V1 header: 19 bytes, V2 header: 15 bytes
//!
//! ## Header Layout (10-byte base)
//!
//! | Offset | Size | Field | Description |
//! |--------|------|-------|-------------|
//! | 0-1 | 2 | magic | "DS" (0x44, 0x53) |
//! | 2 | 1 | version | 1 or 2 |
//! | 3 | 1 | key_size_bits | Encoding key size in **bits** (e.g. 72 = 9 bytes) |
//! | 4-7 | 4 | entry_count | Number of file entries |
//! | 8-9 | 2 | tag_count | Number of tags between header and entries |
//!
//! ## Entry Layout
//!
//! Each entry is `key[ekey_size bytes] + null(1) + key_hash(2 BE) + esize[esize_bytes BE]`.
//!
//! ## Binary Layout
//!
//! Header → Tags → Entries
//!
//! Tags use the same structure as install and download manifest tags
//! (`InstallTag`), with null-terminated name, u16 type, and a bit mask.
//!
//! # Usage
//!
//! ```rust,no_run
//! use cascette_formats::size::{SizeManifest, SizeManifestBuilder};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Build a size manifest
//! let manifest = SizeManifestBuilder::new()
//!     .version(2)
//!     .key_size_bits(72)
//!     .add_entry(vec![0xAA; 9], 0x0001, 1024)
//!     .add_entry(vec![0xBB; 9], 0x0002, 2048)
//!     .build()?;
//!
//! // Serialize to bytes
//! let data = manifest.build()?;
//!
//! // Parse from bytes
//! let parsed = SizeManifest::parse(&data)?;
//! assert_eq!(parsed.entries.len(), 2);
//! # Ok(())
//! # }
//! ```

pub mod builder;
pub mod entry;
pub mod error;
pub mod header;
pub mod manifest;

use crate::install::InstallTag;

/// Size manifest tag type alias
///
/// Size manifests use the same tag structure as install and download manifests.
pub type SizeTag = InstallTag;

// Re-export main types
pub use builder::SizeManifestBuilder;
pub use entry::SizeEntry;
pub use error::{Result, SizeError};
pub use header::{SizeHeader, SizeHeaderV1, SizeHeaderV2};
pub use manifest::SizeManifest;

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_re_exports_accessible() {
        // Verify all public types are accessible through the module re-exports
        let _ = SizeManifestBuilder::new();
        let header = SizeHeader::new_v2(72, 0, 0, 0);
        assert_eq!(header.version(), 2);
        let entry = SizeEntry::new(vec![0x00; 9], 0x0001, 100);
        assert_eq!(entry.esize, 100);
    }

    #[test]
    fn test_basic_workflow() {
        // Build -> serialize -> parse round-trip
        let manifest = SizeManifestBuilder::new()
            .version(1)
            .key_size_bits(72)
            .add_entry(vec![0xAA; 9], 0x0001, 500)
            .add_entry(vec![0xBB; 9], 0x0002, 700)
            .build()
            .expect("Should build manifest");

        assert_eq!(manifest.header.version(), 1);
        assert_eq!(manifest.entries.len(), 2);
        assert_eq!(manifest.header.total_size(), 1200);

        let data = manifest.build().expect("Should serialize");
        let parsed = SizeManifest::parse(&data).expect("Should parse");

        assert_eq!(manifest, parsed);
    }
}
