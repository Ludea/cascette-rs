//! Root file header structures for different versions

use crate::root::{error::Result, version::RootVersion};
use binrw::{BinRead, BinWrite};
use std::io::{Read, Seek, Write};

/// Root file header (V2, V3, V4 only - V1 has no header)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootHeader {
    /// Version 2 header with `MFST` or `TSFM` magic
    V2 {
        /// File counts and structure info
        info: RootHeaderInfo,
    },
    /// Version 3/4 header with extended structure
    V3V4 {
        /// Header size in bytes
        header_size: u32,
        /// Version number (3 or 4)
        version: u32,
        /// File counts and structure info
        info: RootHeaderInfo,
        /// Padding to reach `header_size`
        padding: u32,
    },
}

/// Common header information across versions
#[derive(BinRead, BinWrite, Debug, Clone, PartialEq, Eq)]
#[brw(big)] // Headers use big-endian
pub struct RootHeaderInfo {
    /// Total number of files across all blocks
    pub total_files: u32,
    /// Number of files with name hashes
    pub named_files: u32,
}

/// Magic signatures for root file headers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootMagic {
    /// Standard `MFST` magic
    Mfst,
    /// Alternative `TSFM` magic (byte-swapped `MFST`)
    Tsfm,
}

impl RootMagic {
    /// Magic bytes for `MFST`
    pub const MFST_BYTES: [u8; 4] = *b"MFST";
    /// Magic bytes for `TSFM`
    pub const TSFM_BYTES: [u8; 4] = *b"TSFM";

    /// Convert to bytes
    pub const fn to_bytes(self) -> [u8; 4] {
        match self {
            Self::Mfst => Self::MFST_BYTES,
            Self::Tsfm => Self::TSFM_BYTES,
        }
    }

    /// Parse from bytes
    pub const fn from_bytes(bytes: [u8; 4]) -> Option<Self> {
        match bytes {
            Self::MFST_BYTES => Some(Self::Mfst),
            Self::TSFM_BYTES => Some(Self::Tsfm),
            _ => None,
        }
    }
}

impl RootHeader {
    /// Create V2 header
    pub fn new_v2(total_files: u32, named_files: u32) -> Self {
        Self::V2 {
            info: RootHeaderInfo {
                total_files,
                named_files,
            },
        }
    }

    /// Create V3/V4 header
    pub fn new_v3v4(version: u32, total_files: u32, named_files: u32) -> Self {
        Self::V3V4 {
            header_size: 20, // Standard size
            version,
            info: RootHeaderInfo {
                total_files,
                named_files,
            },
            padding: 0,
        }
    }

    /// Get total files count
    pub const fn total_files(&self) -> u32 {
        match self {
            Self::V2 { info } | Self::V3V4 { info, .. } => info.total_files,
        }
    }

    /// Get named files count
    pub const fn named_files(&self) -> u32 {
        match self {
            Self::V2 { info } | Self::V3V4 { info, .. } => info.named_files,
        }
    }

    /// Get version number
    pub const fn version(&self) -> RootVersion {
        match self {
            Self::V2 { .. } => RootVersion::V2,
            Self::V3V4 { version: 3, .. } => RootVersion::V3,
            Self::V3V4 { version: 4, .. } => RootVersion::V4,
            Self::V3V4 { version, .. } => {
                // Default to V4 for unknown versions >= 4
                if *version >= 4 {
                    RootVersion::V4
                } else {
                    RootVersion::V3
                }
            }
        }
    }

    /// Calculate header size in bytes
    pub const fn size(&self) -> usize {
        match self {
            Self::V2 { .. } => 12, // magic(4) + info(8)
            Self::V3V4 { header_size, .. } => *header_size as usize,
        }
    }

    /// Read header from reader
    pub fn read<R: Read + Seek>(reader: &mut R, version: RootVersion) -> Result<Self> {
        match version {
            RootVersion::V1 => unreachable!("V1 has no header"),
            RootVersion::V2 => {
                // Skip magic (already read for version detection)
                let mut magic_bytes = [0u8; 4];
                reader.read_exact(&mut magic_bytes)?;

                // Read header info
                let info = RootHeaderInfo::read_be(reader)?;

                Ok(Self::V2 { info })
            }
            RootVersion::V3 | RootVersion::V4 => {
                // Skip magic (already read for version detection)
                let mut magic_bytes = [0u8; 4];
                reader.read_exact(&mut magic_bytes)?;

                // Read extended header
                let header_size = u32::read_be(reader)?;
                let version_field = u32::read_be(reader)?;
                let info = RootHeaderInfo::read_be(reader)?;
                let padding = u32::read_be(reader)?;

                Ok(Self::V3V4 {
                    header_size,
                    version: version_field,
                    info,
                    padding,
                })
            }
        }
    }

    /// Write header to writer
    pub fn write<W: Write + Seek>(&self, writer: &mut W) -> Result<()> {
        match self {
            Self::V2 { info } => {
                // Write magic
                writer.write_all(&RootMagic::MFST_BYTES)?;
                // Write header info
                info.write_be(writer)?;
            }
            Self::V3V4 {
                header_size,
                version,
                info,
                padding,
            } => {
                // Write magic
                writer.write_all(&RootMagic::MFST_BYTES)?;
                // Write extended header
                header_size.write_be(writer)?;
                version.write_be(writer)?;
                info.write_be(writer)?;
                padding.write_be(writer)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_root_magic() {
        assert_eq!(RootMagic::Mfst.to_bytes(), *b"MFST");
        assert_eq!(RootMagic::Tsfm.to_bytes(), *b"TSFM");

        assert_eq!(RootMagic::from_bytes(*b"MFST"), Some(RootMagic::Mfst));
        assert_eq!(RootMagic::from_bytes(*b"TSFM"), Some(RootMagic::Tsfm));
        assert_eq!(RootMagic::from_bytes(*b"XXXX"), None);
    }

    #[test]
    fn test_header_info_round_trip() {
        let original = RootHeaderInfo {
            total_files: 123_456,
            named_files: 78_901,
        };

        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        original
            .write_be(&mut cursor)
            .expect("Test operation should succeed");

        let mut cursor = Cursor::new(&buffer);
        let restored = RootHeaderInfo::read_be(&mut cursor).expect("Test operation should succeed");

        assert_eq!(original, restored);
    }

    #[test]
    fn test_v2_header_round_trip() {
        let header = RootHeader::new_v2(123_456, 78_901);

        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        header
            .write(&mut cursor)
            .expect("Test operation should succeed");

        // Should be: magic(4) + total_files(4) + named_files(4) = 12 bytes
        assert_eq!(buffer.len(), 12);
        assert_eq!(&buffer[0..4], b"MFST");

        let mut cursor = Cursor::new(&buffer);
        let restored =
            RootHeader::read(&mut cursor, RootVersion::V2).expect("Test operation should succeed");

        assert_eq!(header, restored);
        assert_eq!(restored.total_files(), 123_456);
        assert_eq!(restored.named_files(), 78_901);
        assert_eq!(restored.version(), RootVersion::V2);
    }

    #[test]
    fn test_v3_header_round_trip() {
        let header = RootHeader::new_v3v4(3, 123_456, 78_901);

        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        header
            .write(&mut cursor)
            .expect("Test operation should succeed");

        // Should be: magic(4) + header_size(4) + version(4) + info(8) + padding(4) = 24 bytes
        assert_eq!(buffer.len(), 24);
        assert_eq!(&buffer[0..4], b"MFST");

        let mut cursor = Cursor::new(&buffer);
        let restored =
            RootHeader::read(&mut cursor, RootVersion::V3).expect("Test operation should succeed");

        assert_eq!(header, restored);
        assert_eq!(restored.total_files(), 123_456);
        assert_eq!(restored.named_files(), 78_901);
        assert_eq!(restored.version(), RootVersion::V3);
    }

    #[test]
    fn test_v4_header_round_trip() {
        let header = RootHeader::new_v3v4(4, 123_456, 78_901);

        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        header
            .write(&mut cursor)
            .expect("Test operation should succeed");

        let mut cursor = Cursor::new(&buffer);
        let restored =
            RootHeader::read(&mut cursor, RootVersion::V4).expect("Test operation should succeed");

        assert_eq!(header, restored);
        assert_eq!(restored.version(), RootVersion::V4);
    }

    #[test]
    fn test_header_size() {
        let v2_header = RootHeader::new_v2(1000, 500);
        assert_eq!(v2_header.size(), 12);

        let v3_header = RootHeader::new_v3v4(3, 1000, 500);
        assert_eq!(v3_header.size(), 20); // header_size field value
    }
}
