//! Root file version detection and management

use crate::root::error::Result;
use std::io::{Read, Seek, SeekFrom};

/// Root file versions across `WoW` expansions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootVersion {
    /// Version 1 (`WoW` 6.0-7.2): No header, interleaved format
    V1,
    /// Version 2 (`WoW` 7.2.5-8.1): `MFST`/`TSFM` header, separated arrays
    V2,
    /// Version 3 (`WoW` 8.2-9.1): Extended header with size/version
    V3,
    /// Version 4 (`WoW` 9.1+): 40-bit content flags (same structure as V3)
    V4,
}

impl RootVersion {
    /// Detect root file version from data
    pub fn detect<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        let start_pos = reader.stream_position()?;

        // Read first 12 bytes to check for magic and header structure
        let mut magic_buffer = [0u8; 4];
        reader.read_exact(&mut magic_buffer)?;

        let version = match &magic_buffer {
            b"MFST" => {
                // Could be V2 or V3/V4 - check header size field
                let header_size = {
                    let mut size_bytes = [0u8; 4];
                    reader.read_exact(&mut size_bytes)?;
                    u32::from_be_bytes(size_bytes)
                };

                // V3/V4 have header_size field (typically 20-24 bytes)
                // V2 interprets this field as total_files count (usually > 100)
                //
                // The heuristic: V3/V4 header_size is usually exactly 20 or 24
                // V2 total_files is usually > 100 for real files
                // But for minimal test files with 1-10 files, we need better detection

                // Check if this could be a valid V3/V4 header size
                if header_size == 20 || header_size == 24 || header_size == 28 {
                    // Very likely V3/V4 - read version field to confirm
                    let version_field = {
                        let mut version_bytes = [0u8; 4];
                        reader.read_exact(&mut version_bytes)?;
                        u32::from_be_bytes(version_bytes)
                    };

                    // V3 has version=3, V4 has version=4 (or higher)
                    if version_field == 3 {
                        Self::V3
                    } else if version_field >= 4 {
                        Self::V4
                    } else {
                        // Unexpected version, assume V2
                        Self::V2
                    }
                } else {
                    // Not a standard V3/V4 header size, must be V2
                    Self::V2
                }
            }
            b"TSFM" => {
                // Alternative V2 magic (byte-swapped MFST)
                Self::V2
            }
            _ => {
                // No recognized magic = V1
                Self::V1
            }
        };

        // Reset reader position
        reader.seek(SeekFrom::Start(start_pos))?;

        Ok(version)
    }

    /// Check if version supports named files (name hashes)
    pub const fn supports_named_files(self) -> bool {
        match self {
            // All versions support named files (V1 always, V2-V4 optional based on flags)
            Self::V1 | Self::V2 | Self::V3 | Self::V4 => true,
        }
    }

    /// Check if version uses separated arrays (vs interleaved)
    pub const fn uses_separated_arrays(self) -> bool {
        match self {
            Self::V1 => false,                      // Interleaved format
            Self::V2 | Self::V3 | Self::V4 => true, // Separated arrays
        }
    }

    /// Check if version has header
    pub const fn has_header(self) -> bool {
        match self {
            Self::V1 => false,
            Self::V2 | Self::V3 | Self::V4 => true,
        }
    }

    /// Check if version supports extended content flags (40-bit)
    pub const fn supports_extended_content_flags(self) -> bool {
        match self {
            Self::V1 | Self::V2 | Self::V3 => false,
            Self::V4 => true,
        }
    }

    /// Get content flags size in bytes
    pub const fn content_flags_size(self) -> usize {
        match self {
            Self::V1 | Self::V2 | Self::V3 => 4, // 32-bit
            Self::V4 => 5,                       // 40-bit (32 + 8 bits)
        }
    }

    /// Convert to numeric representation
    pub const fn to_u32(self) -> u32 {
        match self {
            Self::V1 => 1,
            Self::V2 => 2,
            Self::V3 => 3,
            Self::V4 => 4,
        }
    }

    /// Create from numeric representation
    pub const fn from_u32(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::V1),
            2 => Some(Self::V2),
            3 => Some(Self::V3),
            4 => Some(Self::V4),
            _ => None,
        }
    }
}

impl std::fmt::Display for RootVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::V1 => write!(f, "V1 (WoW 6.0-7.2)"),
            Self::V2 => write!(f, "V2 (WoW 7.2.5-8.1)"),
            Self::V3 => write!(f, "V3 (WoW 8.2-9.1)"),
            Self::V4 => write!(f, "V4 (WoW 9.1+)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_detect_v1() {
        // V1 has no magic header, just starts with block data
        let data = vec![
            0x10, 0x00, 0x00, 0x00, // num_records (little-endian)
            0x00, 0x00, 0x00, 0x00, // content flags
            0xFF, 0xFF, 0xFF, 0xFF, // locale flags
        ];

        let mut cursor = Cursor::new(&data);
        let version = RootVersion::detect(&mut cursor).expect("Test operation should succeed");
        assert_eq!(version, RootVersion::V1);
    }

    #[test]
    fn test_detect_v2() {
        // V2 has MFST magic with large "header_size" (actually total_files)
        let data = vec![
            b'M', b'F', b'S', b'T', // magic
            0x00, 0x01, 0x00, 0x00, // total_files = 65_536 (big-endian)
            0x00, 0x00, 0x80, 0x00, // named_files = 32_768 (big-endian)
        ];

        let mut cursor = Cursor::new(&data);
        let version = RootVersion::detect(&mut cursor).expect("Test operation should succeed");
        assert_eq!(version, RootVersion::V2);
    }

    #[test]
    fn test_detect_v3() {
        // V3 has MFST magic with small header_size and version < 4
        let data = vec![
            b'M', b'F', b'S', b'T', // magic
            0x00, 0x00, 0x00, 0x14, // header_size = 20 (big-endian)
            0x00, 0x00, 0x00, 0x03, // version = 3 (big-endian)
            0x00, 0x01, 0x00, 0x00, // total_files
            0x00, 0x00, 0x80, 0x00, // named_files
            0x00, 0x00, 0x00, 0x00, // padding
        ];

        let mut cursor = Cursor::new(&data);
        let version = RootVersion::detect(&mut cursor).expect("Test operation should succeed");
        assert_eq!(version, RootVersion::V3);
    }

    #[test]
    fn test_detect_v4() {
        // V4 has MFST magic with small header_size and version >= 4
        let data = vec![
            b'M', b'F', b'S', b'T', // magic
            0x00, 0x00, 0x00, 0x14, // header_size = 20 (big-endian)
            0x00, 0x00, 0x00, 0x04, // version = 4 (big-endian)
            0x00, 0x01, 0x00, 0x00, // total_files
            0x00, 0x00, 0x80, 0x00, // named_files
            0x00, 0x00, 0x00, 0x00, // padding
        ];

        let mut cursor = Cursor::new(&data);
        let version = RootVersion::detect(&mut cursor).expect("Test operation should succeed");
        assert_eq!(version, RootVersion::V4);
    }

    #[test]
    fn test_detect_tsfm() {
        // TSFM is alternative V2 magic (byte-swapped MFST)
        let data = vec![
            b'T', b'S', b'F', b'M', // magic
            0x00, 0x01, 0x00, 0x00, // total_files
            0x00, 0x00, 0x80, 0x00, // named_files
        ];

        let mut cursor = Cursor::new(&data);
        let version = RootVersion::detect(&mut cursor).expect("Test operation should succeed");
        assert_eq!(version, RootVersion::V2);
    }

    #[test]
    fn test_version_properties() {
        assert!(!RootVersion::V1.has_header());
        assert!(RootVersion::V2.has_header());
        assert!(RootVersion::V3.has_header());
        assert!(RootVersion::V4.has_header());

        assert!(!RootVersion::V1.uses_separated_arrays());
        assert!(RootVersion::V2.uses_separated_arrays());
        assert!(RootVersion::V3.uses_separated_arrays());
        assert!(RootVersion::V4.uses_separated_arrays());

        assert!(!RootVersion::V1.supports_extended_content_flags());
        assert!(!RootVersion::V2.supports_extended_content_flags());
        assert!(!RootVersion::V3.supports_extended_content_flags());
        assert!(RootVersion::V4.supports_extended_content_flags());

        assert_eq!(RootVersion::V1.content_flags_size(), 4);
        assert_eq!(RootVersion::V2.content_flags_size(), 4);
        assert_eq!(RootVersion::V3.content_flags_size(), 4);
        assert_eq!(RootVersion::V4.content_flags_size(), 5);
    }

    #[test]
    fn test_numeric_conversion() {
        for version in [
            RootVersion::V1,
            RootVersion::V2,
            RootVersion::V3,
            RootVersion::V4,
        ] {
            let numeric = version.to_u32();
            let restored = RootVersion::from_u32(numeric).expect("Test operation should succeed");
            assert_eq!(version, restored);
        }

        assert_eq!(RootVersion::from_u32(0), None);
        assert_eq!(RootVersion::from_u32(5), None);
    }
}
