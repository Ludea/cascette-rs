//! Index file (.idx) management
//!
//! Index files map content keys to locations within data archives.

#[cfg(test)]
#[allow(unused_imports)]
use crate::validation::BinaryFormatValidator;
use crate::{Result, StorageError};
use binrw::Endian;
use binrw::{BinRead, BinReaderExt, BinResult, BinWrite, BinWriterExt};
use cascette_crypto::EncodingKey;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info, warn};

/// Custom binrw parser for archive location (5 bytes: 1 high + 4 packed)
fn parse_archive_location<R: std::io::Read + std::io::Seek>(
    reader: &mut R,
    _endian: Endian,
    _args: (),
) -> BinResult<ArchiveLocation> {
    // Read high byte of archive ID
    let index_high = u16::from(reader.read_be::<u8>()?);

    // Read 4-byte packed field (big-endian: archive ID low bits + offset)
    let index_low = reader.read_be::<u32>()?;

    // Extract archive ID: high byte shifted left by 2, plus top 2 bits of low word
    let archive_id = (index_high << 2) | u16::try_from(index_low >> 30).unwrap_or(0);

    // Extract offset: bottom 30 bits of low word
    let archive_offset = index_low & 0x3FFF_FFFF;

    Ok(ArchiveLocation {
        archive_id,
        archive_offset,
    })
}

/// Custom binrw writer for archive location (5 bytes: 1 high + 4 packed)
fn write_archive_location<W: std::io::Write + std::io::Seek>(
    location: &ArchiveLocation,
    writer: &mut W,
    _endian: Endian,
    _args: (),
) -> BinResult<()> {
    // Write high byte of archive ID
    let index_high =
        u8::try_from(location.archive_id >> 2).map_err(|e| binrw::Error::AssertFail {
            pos: 0,
            message: format!("Archive ID too large: {e}"),
        })?;
    writer.write_be(&index_high)?;

    // Pack low bits of archive ID with offset
    let archive_low = u32::from(location.archive_id & 0x03);
    let index_low = (archive_low << 30) | (location.archive_offset & 0x3FFF_FFFF);

    // Write 4-byte packed field (big-endian)
    writer.write_be(&index_low)?;

    Ok(())
}

/// IDX Journal header for local CASC storage
///
/// Note: IDX Journal v7 uses little-endian headers, unlike most NGDP formats
/// which use big-endian. This was confirmed by testing against `WoW` Classic
/// 1.15.2.55140 .idx files.
#[derive(Debug, Clone, BinRead, BinWrite)]
#[brw(little)]
pub struct IndexHeader {
    /// Size of header data section
    pub data_size: u32,
    /// Jenkins hash for validation
    pub data_hash: u32,
    /// Journal version
    pub version: u16,
    /// Bucket ID (0x00-0xFF)
    pub bucket: u8,
    /// Unused padding
    pub unused: u8,
    /// Size field bytes (4 for standard)
    pub length_size: u8,
    /// Location field bytes (5 = 1 archive + 4 offset)
    pub location_size: u8,
    /// Key field bytes (9 or 16)
    pub key_size: u8,
    /// Segment size bits (30 for standard)
    pub segment_bits: u8,
}

/// Entry in an index file (18-byte IDX Journal format)
#[derive(Debug, Clone, PartialEq, Eq, BinRead, BinWrite)]
#[brw(big)] // NGDP uses big-endian by default
pub struct IndexEntry {
    /// Truncated encoding key (first 9 bytes)
    pub key: [u8; 9],

    /// Archive location data (5 bytes: 1 byte high archive ID + 4 bytes packed low/offset)
    #[br(parse_with = parse_archive_location)]
    #[bw(write_with = write_archive_location)]
    pub archive_location: ArchiveLocation,

    /// Size of the content (4 bytes, little-endian for legacy compatibility)
    #[brw(little)]
    pub size: u32,
}

/// Archive location data combining archive ID and offset
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveLocation {
    /// Archive file number (data.XXX)
    pub archive_id: u16,
    /// Offset within the archive
    pub archive_offset: u32,
}

impl IndexEntry {
    /// Create new `IndexEntry` with the given parameters
    pub const fn new(key: [u8; 9], archive_id: u16, archive_offset: u32, size: u32) -> Self {
        Self {
            key,
            archive_location: ArchiveLocation {
                archive_id,
                archive_offset,
            },
            size,
        }
    }

    /// Get archive ID from location
    pub const fn archive_id(&self) -> u16 {
        self.archive_location.archive_id
    }

    /// Get archive offset from location
    pub const fn archive_offset(&self) -> u32 {
        self.archive_location.archive_offset
    }

    /// Create from raw packed data (IDX Journal format) - legacy compatibility
    ///
    /// # Errors
    ///
    /// Returns error if data is malformed or insufficient
    pub fn from_packed(
        data: &[u8],
        _key_bytes: usize,
        _offset_bits: usize,
        _size_bits: usize,
    ) -> Result<Self> {
        // For local CASC IDX files, use fixed 18-byte format
        if data.len() < 18 {
            return Err(StorageError::Index("Entry data too small".to_string()));
        }

        // Parse using binrw - create a cursor and read
        let mut cursor = Cursor::new(data);
        match Self::read_be(&mut cursor) {
            Ok(entry) => {
                // Skip empty entries
                if entry.key == [0u8; 9] {
                    return Err(StorageError::Index("Empty entry".to_string()));
                }
                Ok(entry)
            }
            Err(e) => Err(StorageError::Index(format!("Failed to parse entry: {e}"))),
        }
    }

    /// Pack entry to raw data - legacy compatibility
    pub fn to_packed(&self, _key_bytes: usize, _offset_bits: usize, _size_bits: usize) -> Vec<u8> {
        // Use binrw to serialize
        let mut data = Vec::new();
        let mut cursor = Cursor::new(&mut data);

        // Write using binrw
        if let Err(e) = self.write_be(&mut cursor) {
            eprintln!("Warning: Failed to serialize entry with binrw: {e}");
            // Fallback to ensure we return something
            return vec![0; 18];
        }

        data
    }
}

/// Index file manager
pub struct IndexManager {
    /// Map of index ID to loaded index data
    indices: BTreeMap<u8, IndexFile>,
    /// Directory containing index files
    base_path: PathBuf,
}

/// Individual index file data
struct IndexFile {
    /// Index file header
    header: IndexHeader,
    /// Sorted entries for binary search
    entries: Vec<IndexEntry>,
}

impl IndexManager {
    /// Create new index manager for a directory
    pub fn new(base_path: impl AsRef<Path>) -> Self {
        Self {
            indices: BTreeMap::new(),
            base_path: base_path.as_ref().to_path_buf(),
        }
    }

    /// Load all index files from the directory
    ///
    /// # Errors
    ///
    /// Returns error if directory cannot be read or index files cannot be loaded
    pub async fn load_all(&mut self) -> Result<()> {
        info!("Loading index files from {:?}", self.base_path);

        // Find all .idx files
        let mut entries = fs::read_dir(&self.base_path)
            .await
            .map_err(|e| StorageError::Index(format!("Failed to read directory: {e}")))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| StorageError::Index(format!("Failed to read entry: {e}")))?
        {
            let path = entry.path();

            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                // Parse index file names using official CASC format
                if let Some((bucket, version)) = Self::parse_index_filename(name) {
                    debug!(
                        "Loading index file bucket {:02x} version {:06x} from {:?}",
                        bucket, version, path
                    );
                    self.load_index(bucket, &path)?;
                }
            }
        }

        info!("Loaded {} index files", self.indices.len());
        Ok(())
    }

    /// Load a specific index file
    ///
    /// # Errors
    ///
    /// Returns error if file cannot be opened, read, or parsed
    pub fn load_index(&mut self, id: u8, path: &Path) -> Result<()> {
        let file = File::open(path)
            .map_err(|e| StorageError::Index(format!("Failed to open index: {e}")))?;
        let mut reader = BufReader::new(file);

        // Read header (little-endian for IDX Journal v7)
        let header: IndexHeader = reader
            .read_le()
            .map_err(|e| StorageError::Index(format!("Failed to read header: {e}")))?;

        // Validate header
        if header.version != 7 {
            warn!("Unexpected index version: {}", header.version);
        }
        if header.key_size != 9 && header.key_size != 16 {
            return Err(StorageError::Index(format!(
                "Invalid key size: {}",
                header.key_size
            )));
        }

        // Skip block table
        let block_count = (header.data_size - 8) / 8;
        for _ in 0..block_count {
            let _block = reader
                .read_be::<u64>()
                .map_err(|e| StorageError::Index(format!("Failed to read block: {e}")))?;
        }

        // Align to 16-byte boundary
        let pos = reader
            .stream_position()
            .map_err(|e| StorageError::Index(format!("Failed to get position: {e}")))?;
        let padding = (16 - (pos % 16)) % 16;
        if padding > 0 {
            reader
                .seek(SeekFrom::Current(i64::try_from(padding).map_err(|e| {
                    StorageError::Index(format!("Seek offset too large: {e}"))
                })?))
                .map_err(|e| StorageError::Index(format!("Failed to seek: {e}")))?;
        }

        // Read data section header (big-endian per NGDP standard)
        let data_section_size = reader
            .read_be::<u32>()
            .map_err(|e| StorageError::Index(format!("Failed to read data size: {e}")))?;
        let _data_section_hash = reader
            .read_be::<u32>()
            .map_err(|e| StorageError::Index(format!("Failed to read data hash: {e}")))?;

        // Calculate entry parameters
        let entry_size = (header.key_size + header.location_size + header.length_size) as usize;

        // Read all entries
        let mut entries = Vec::new();
        let mut data = vec![0u8; data_section_size as usize];
        reader
            .read_exact(&mut data)
            .map_err(|e| StorageError::Index(format!("Failed to read data: {e}")))?;

        // Parse entries
        let mut offset = 0;
        while offset + entry_size <= data.len() {
            let entry_data = &data[offset..offset + entry_size];

            // Check if entry is valid (non-zero)
            if entry_data.iter().any(|&b| b != 0) {
                // Use fixed parsing for IDX Journal format
                let entry = IndexEntry::from_packed(
                    entry_data,
                    header.key_size as usize,
                    30, // Standard segment bits
                    32, // Standard size bits
                )?;
                entries.push(entry);
            }

            offset += entry_size;
        }

        // Sort entries by key for binary search
        entries.sort_by_key(|e| e.key);

        debug!("Loaded {} entries from index {:02x}", entries.len(), id);
        self.indices.insert(id, IndexFile { header, entries });

        Ok(())
    }

    /// Look up an encoding key in the indices
    pub fn lookup(&self, key: &EncodingKey) -> Option<IndexEntry> {
        let key_bytes = key.as_bytes();

        // Determine which index to search using official CASC bucket algorithm
        let index_id = Self::get_bucket_index(key_bytes);

        self.indices.get(&index_id).and_then(|index| {
            // Create search key (first 9 bytes)
            let mut search_key = [0u8; 9];
            search_key[..9.min(key_bytes.len())]
                .copy_from_slice(&key_bytes[..9.min(key_bytes.len())]);

            // Binary search for the key
            index
                .entries
                .binary_search_by_key(&search_key, |e| e.key)
                .ok()
                .map(|idx| index.entries[idx].clone())
        })
    }

    /// Add a new entry to the appropriate index
    ///
    /// # Errors
    ///
    /// Returns error if index allocation fails or entry cannot be added
    pub fn add_entry(
        &mut self,
        key: &EncodingKey,
        archive_id: u16,
        archive_offset: u32,
        size: u32,
    ) -> Result<()> {
        let key_bytes = key.as_bytes();
        let index_id = Self::get_bucket_index(key_bytes);

        // Create truncated key
        let mut truncated_key = [0u8; 9];
        truncated_key[..9.min(key_bytes.len())]
            .copy_from_slice(&key_bytes[..9.min(key_bytes.len())]);

        let entry = IndexEntry::new(truncated_key, archive_id, archive_offset, size);

        // Get or create index
        let index = self.indices.entry(index_id).or_insert_with(|| IndexFile {
            header: IndexHeader {
                data_size: 16, // Minimal header
                data_hash: 0,
                version: 7,
                bucket: index_id,
                unused: 0,
                length_size: 4,
                location_size: 5,
                key_size: 9,
                segment_bits: 30,
            },
            entries: Vec::new(),
        });

        // Insert entry maintaining sort order
        match index
            .entries
            .binary_search_by_key(&truncated_key, |e| e.key)
        {
            Ok(idx) => {
                // Update existing entry
                index.entries[idx] = entry;
            }
            Err(idx) => {
                // Insert new entry
                index.entries.insert(idx, entry);
            }
        }

        Ok(())
    }

    /// Save all modified indices to disk
    ///
    /// # Errors
    ///
    /// Returns error if index files cannot be created or written
    pub fn save_all(&self) -> Result<()> {
        for (&id, index) in &self.indices {
            // Use version 1 for new index files - in production this would be incremented
            let filename = Self::generate_index_filename(id, 1);
            let path = self.base_path.join(filename);
            Self::save_index(id, index, &path)?;
        }
        Ok(())
    }

    /// Get bucket index for a key using official CASC algorithm
    /// Based on wowdev.wiki CASC specification
    fn get_bucket_index(key: &[u8]) -> u8 {
        if key.len() < 9 {
            return 0;
        }

        // XOR together each byte in the first 9 bytes of the key
        let hash = key[0] ^ key[1] ^ key[2] ^ key[3] ^ key[4] ^ key[5] ^ key[6] ^ key[7] ^ key[8];

        // XOR the upper and lower nibbles
        (hash & 0x0F) ^ (hash >> 4)
    }

    /// Generate official CASC index filename pattern
    /// Format: {bucket:02x}{version:06x}.idx
    fn generate_index_filename(bucket: u8, version: u32) -> String {
        format!("{bucket:02x}{version:06x}.idx")
    }

    /// Parse bucket and version from official CASC index filename
    fn parse_index_filename(filename: &str) -> Option<(u8, u32)> {
        if filename.len() != 12
            || !std::path::Path::new(filename)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("idx"))
        {
            return None;
        }

        let bucket = u8::from_str_radix(&filename[0..2], 16).ok()?;
        let version = u32::from_str_radix(&filename[2..8], 16).ok()?;

        Some((bucket, version))
    }

    /// Save a specific index to disk
    fn save_index(id: u8, index: &IndexFile, path: &Path) -> Result<()> {
        let file = File::create(path)
            .map_err(|e| StorageError::Index(format!("Failed to create index: {e}")))?;
        let mut writer = BufWriter::new(file);

        // Calculate data size
        let entry_size = (index.header.key_size
            + index.header.location_size
            + index.header.length_size) as usize;
        let _data_section_size = index.entries.len() * entry_size;

        // Write updated header (little-endian for IDX Journal v7)
        let header = index.header.clone();
        // Note: data_size would need to include block table, but we'll keep it simple
        writer
            .write_le(&header)
            .map_err(|e| StorageError::Index(format!("Failed to write header: {e}")))?;

        // Write entries
        for entry in &index.entries {
            let packed = entry.to_packed(
                index.header.key_size as usize,
                30, // Standard segment bits
                32, // Standard size bits
            );
            writer
                .write_all(&packed)
                .map_err(|e| StorageError::Index(format!("Failed to write entry: {e}")))?;
        }

        writer
            .flush()
            .map_err(|e| StorageError::Index(format!("Failed to flush index: {e}")))?;

        debug!(
            "Saved index {:02x} with {} entries",
            id,
            index.entries.len()
        );
        Ok(())
    }

    /// Get statistics about loaded indices
    pub fn stats(&self) -> IndexStats {
        let total_entries: usize = self.indices.values().map(|idx| idx.entries.len()).sum();

        IndexStats {
            index_count: self.indices.len(),
            total_entries,
        }
    }
}

/// Statistics about loaded indices
#[derive(Debug, Clone)]
pub struct IndexStats {
    /// Number of loaded index files
    pub index_count: usize,
    /// Total number of entries across all indices
    pub total_entries: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_index_header_round_trip() {
        let original = IndexHeader {
            data_size: 0x1234_5678,
            data_hash: 0xABCD_EF00,
            version: 7,
            bucket: 0xFF,
            unused: 0,
            length_size: 4,
            location_size: 5,
            key_size: 9,
            segment_bits: 30,
        };

        // Serialize using little-endian (IDX Journal v7 format)
        let mut data = Vec::new();
        let mut cursor = Cursor::new(&mut data);
        original
            .write_le(&mut cursor)
            .expect("Failed to write to cursor in test");

        // Debug: check actual size
        println!("IndexHeader serialized to {} bytes", data.len());
        // IndexHeader should be 16 bytes (4+4+2+1+1+1+1+1+1 = 16)
        assert_eq!(data.len(), 16);

        // Deserialize back
        let mut cursor = Cursor::new(&data[..]);
        let parsed = IndexHeader::read_le(&mut cursor).expect("Failed to read from cursor in test");

        // Verify round-trip
        assert_eq!(original.data_size, parsed.data_size);
        assert_eq!(original.data_hash, parsed.data_hash);
        assert_eq!(original.version, parsed.version);
        assert_eq!(original.bucket, parsed.bucket);
        assert_eq!(original.unused, parsed.unused);
        assert_eq!(original.length_size, parsed.length_size);
        assert_eq!(original.location_size, parsed.location_size);
        assert_eq!(original.key_size, parsed.key_size);
        assert_eq!(original.segment_bits, parsed.segment_bits);
    }

    #[test]
    fn test_index_entry_round_trip() {
        let original = IndexEntry::new(
            [0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x00],
            0x0234,      // Archive ID (10 bits: up to 0x3FF = 1023)
            0x1678_9ABC, // Archive offset (30 bits: up to 0x3FFFFFFF)
            0x8765_4321, // Size
        );

        // Serialize using big-endian
        let mut data = Vec::new();
        let mut cursor = Cursor::new(&mut data);
        original
            .write_be(&mut cursor)
            .expect("Failed to write to cursor in test");

        // Verify expected byte layout (18 bytes total)
        assert_eq!(data.len(), 18);

        // Verify key bytes (first 9 bytes)
        assert_eq!(
            &data[0..9],
            &[0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x00]
        );

        // Deserialize back
        let mut cursor = Cursor::new(&data[..]);
        let parsed = IndexEntry::read_be(&mut cursor).expect("Failed to read from cursor in test");

        // Verify round-trip
        assert_eq!(original.key, parsed.key);
        assert_eq!(original.archive_id(), parsed.archive_id());
        assert_eq!(original.archive_offset(), parsed.archive_offset());
        assert_eq!(original.size, parsed.size);
    }

    #[test]
    fn test_archive_location_packing() {
        // Test various archive ID and offset combinations
        // Archive ID: 10 bits max (0x3FF = 1023)
        // Archive offset: 30 bits max (0x3FFFFFFF = 1073741823)
        let test_cases = vec![
            (0x0000, 0x0000_0000), // Minimum values
            (0x03FF, 0x3FFF_FFFF), // Maximum values (10 bits archive, 30 bits offset)
            (0x0001, 0x0000_0001), // Small values
            (0x0100, 0x1234_5678), // Mid-range values
            (0x0255, 0x2AAA_AAAA), // Pattern test
        ];

        for (archive_id, archive_offset) in test_cases {
            let original = IndexEntry::new([0; 9], archive_id, archive_offset, 0x1234_5678);

            // Round-trip test
            let mut data = Vec::new();
            let mut cursor = Cursor::new(&mut data);
            original
                .write_be(&mut cursor)
                .expect("Failed to write to cursor in test");

            let mut cursor = Cursor::new(&data[..]);
            let parsed =
                IndexEntry::read_be(&mut cursor).expect("Failed to read from cursor in test");

            assert_eq!(
                original.archive_id(),
                parsed.archive_id(),
                "Archive ID mismatch for input {archive_id:#x}"
            );
            assert_eq!(
                original.archive_offset(),
                parsed.archive_offset(),
                "Archive offset mismatch for input {archive_offset:#x}"
            );
        }
    }

    #[test]
    fn test_legacy_compatibility() {
        // Test that from_packed still works for backwards compatibility
        let original = IndexEntry::new(
            [0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x00],
            0x0123,      // Archive ID (10 bits max)
            0x0567_89AB, // Archive offset (30 bits max)
            0x8765_4321,
        );

        // Use to_packed to get legacy format
        let packed_data = original.to_packed(9, 35, 32);
        assert_eq!(packed_data.len(), 18);

        // Use from_packed to read it back
        let parsed = IndexEntry::from_packed(&packed_data, 9, 35, 32)
            .expect("Failed to parse packed data in test");

        assert_eq!(original.key, parsed.key);
        assert_eq!(original.archive_id(), parsed.archive_id());
        assert_eq!(original.archive_offset(), parsed.archive_offset());
        assert_eq!(original.size, parsed.size);
    }

    #[test]
    fn test_empty_entry_rejection() {
        // Test that empty entries are properly rejected
        let empty_data = [0u8; 18];
        let result = IndexEntry::from_packed(&empty_data, 9, 35, 32);

        assert!(result.is_err());
        assert!(
            result
                .expect_err("Test operation should fail")
                .to_string()
                .contains("Empty entry")
        );
    }

    #[test]
    fn test_insufficient_data_rejection() {
        // Test that insufficient data is properly rejected
        let short_data = [0u8; 10]; // Too short
        let result = IndexEntry::from_packed(&short_data, 9, 35, 32);

        assert!(result.is_err());
        assert!(
            result
                .expect_err("Test operation should fail")
                .to_string()
                .contains("too small")
        );
    }

    #[test]
    fn test_casc_bucket_algorithm() {
        // Test the official CASC bucket algorithm against known values
        let test_cases = vec![
            // Test case: all zeros should map to bucket 0
            ([0u8; 16], 0),
            // Test case: alternating pattern
            (
                [
                    0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x00, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x00,
                ],
                0,
            ),
            // Test case: sequential bytes (1^2^3^4^5^6^7^8^9 = 1, (1&0x0F)^(1>>4) = 1)
            (
                [
                    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x00, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x00,
                ],
                1,
            ),
            // Test case: maximum values
            ([0xFF; 16], 0),
        ];

        for (key, expected_bucket) in test_cases {
            let bucket = IndexManager::get_bucket_index(&key);
            assert_eq!(
                bucket,
                expected_bucket,
                "Bucket mismatch for key {:02x?}: got {}, expected {}",
                &key[0..9],
                bucket,
                expected_bucket
            );

            // Verify bucket is in valid range (0-15)
            assert!(bucket <= 15, "Bucket {bucket} exceeds maximum of 15");
        }
    }

    #[test]
    fn test_casc_filename_generation() {
        let test_cases = vec![
            (0, 1, "00000001.idx"),
            (15, 0x0012_3456, "0f123456.idx"),
            (7, 0x00AB_CDEF, "07abcdef.idx"),
        ];

        for (bucket, version, expected) in test_cases {
            let filename = IndexManager::generate_index_filename(bucket, version);
            assert_eq!(
                filename, expected,
                "Filename mismatch for bucket {bucket}, version 0x{version:x}"
            );
        }
    }

    #[test]
    fn test_casc_filename_parsing() {
        let test_cases = vec![
            ("00000001.idx", Some((0, 1))),
            ("0f123456.idx", Some((15, 0x0012_3456))),
            ("07abcdef.idx", Some((7, 0x00AB_CDEF))),
            ("invalid.idx", None),
            ("00000001.txt", None),
            ("short.idx", None),
        ];

        for (filename, expected) in test_cases {
            let result = IndexManager::parse_index_filename(filename);
            assert_eq!(result, expected, "Parse mismatch for filename: {filename}");
        }
    }

    #[test]
    fn test_bucket_distribution() {
        // Test that the bucket algorithm provides reasonable distribution
        let mut bucket_counts = [0u32; 16];

        // Generate test keys and count bucket distribution
        for i in 0..1000u32 {
            let mut key = [0u8; 16];
            key[0..4].copy_from_slice(&i.to_be_bytes());
            key[4..8].copy_from_slice(&(i.wrapping_mul(37)).to_be_bytes());
            key[8] = u8::try_from(i % 256).unwrap_or(0);

            let bucket = IndexManager::get_bucket_index(&key);
            bucket_counts[bucket as usize] += 1;
        }

        // Verify all buckets are used (no bucket should be completely empty)
        for (bucket, &count) in bucket_counts.iter().enumerate() {
            assert!(count > 0, "Bucket {bucket} was never used");
        }

        // Verify reasonable distribution (no bucket should have more than 2x average)
        let average = 1000.0 / 16.0;
        for (bucket, &count) in bucket_counts.iter().enumerate() {
            let ratio = f64::from(count) / average;
            assert!(
                ratio < 2.0,
                "Bucket {bucket} has too many entries: {count} ({ratio}x average)"
            );
        }
    }
}

// Validation implementations for round-trip testing
#[cfg(test)]
mod validation_impls {
    use super::*;
    use crate::validation::BinaryFormatValidator;

    impl PartialEq for IndexHeader {
        fn eq(&self, other: &Self) -> bool {
            self.data_size == other.data_size
                && self.data_hash == other.data_hash
                && self.version == other.version
                && self.bucket == other.bucket
                && self.unused == other.unused
                && self.length_size == other.length_size
                && self.location_size == other.location_size
                && self.key_size == other.key_size
                && self.segment_bits == other.segment_bits
        }
    }

    impl BinaryFormatValidator for IndexHeader {
        fn generate_valid_instance() -> Self {
            Self {
                data_size: 0x1234_5678,
                data_hash: 0xABCD_EF00,
                version: 7, // Standard CASC version
                bucket: 0x0F,
                unused: 0,
                length_size: 4,
                location_size: 5,
                key_size: 9,
                segment_bits: 30,
            }
        }

        fn generate_edge_cases() -> Vec<Self> {
            vec![
                // Minimum values
                Self {
                    data_size: 0,
                    data_hash: 0,
                    version: 0,
                    bucket: 0,
                    unused: 0,
                    length_size: 0,
                    location_size: 0,
                    key_size: 0,
                    segment_bits: 0,
                },
                // Maximum values
                Self {
                    data_size: u32::MAX,
                    data_hash: u32::MAX,
                    version: u16::MAX,
                    bucket: u8::MAX,
                    unused: u8::MAX,
                    length_size: u8::MAX,
                    location_size: u8::MAX,
                    key_size: u8::MAX,
                    segment_bits: u8::MAX,
                },
                // Standard CASC values
                Self {
                    data_size: 16,
                    data_hash: 0x1234_5678,
                    version: 7,
                    bucket: 0x08,
                    unused: 0,
                    length_size: 4,
                    location_size: 5,
                    key_size: 9,
                    segment_bits: 30,
                },
                // 16-byte key variant
                Self {
                    data_size: 16,
                    data_hash: 0x8765_4321,
                    version: 7,
                    bucket: 0x0C,
                    unused: 0,
                    length_size: 4,
                    location_size: 5,
                    key_size: 16, // 16-byte keys
                    segment_bits: 30,
                },
            ]
        }

        fn validate_serialized_data(&self, data: &[u8]) -> Result<()> {
            if data.len() != 16 {
                return Err(StorageError::InvalidFormat(format!(
                    "IndexHeader should be 16 bytes, got {}",
                    data.len()
                )));
            }

            // Check little-endian byte order by validating data_size field
            let expected_data_size_bytes = self.data_size.to_le_bytes();
            if data[0..4] != expected_data_size_bytes {
                return Err(StorageError::InvalidFormat(
                    "Data size field not in little-endian format".to_string(),
                ));
            }

            Ok(())
        }
    }

    impl BinaryFormatValidator for IndexEntry {
        fn generate_valid_instance() -> Self {
            Self {
                key: [0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x00],
                archive_location: ArchiveLocation {
                    archive_id: 0x0123,                        // 10-bit max: 0x3FF
                    archive_offset: 0x1234_5678 & 0x3FFF_FFFF, // 30-bit max
                },
                size: 0x8765_4321,
            }
        }

        fn generate_edge_cases() -> Vec<Self> {
            vec![
                // Minimum values (skip because empty entries are rejected)
                Self {
                    key: [0x01, 0, 0, 0, 0, 0, 0, 0, 0], // Non-zero key to avoid empty entry rejection
                    archive_location: ArchiveLocation {
                        archive_id: 0,
                        archive_offset: 0,
                    },
                    size: 0,
                },
                // Maximum archive ID and offset (10-bit and 30-bit limits)
                Self {
                    key: [0xFF; 9],
                    archive_location: ArchiveLocation {
                        archive_id: 0x03FF,          // Maximum 10-bit value
                        archive_offset: 0x3FFF_FFFF, // Maximum 30-bit value
                    },
                    size: u32::MAX,
                },
                // Boundary values for archive ID (test bit packing)
                Self {
                    key: [0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x55, 0xAA],
                    archive_location: ArchiveLocation {
                        archive_id: 0x0100,          // 256
                        archive_offset: 0x1000_0000, // 2^28
                    },
                    size: 0x1234_5678,
                },
                // Test edge case for bit boundaries
                Self {
                    key: [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0x12],
                    archive_location: ArchiveLocation {
                        archive_id: 0x02AA,          // Pattern test
                        archive_offset: 0x2AAA_AAAA, // Pattern test
                    },
                    size: 0x5555_AAAA,
                },
                // Standard realistic values
                Self {
                    key: [0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE, 0x00],
                    archive_location: ArchiveLocation {
                        archive_id: 42,
                        archive_offset: 0x0010_0000, // 1MB offset
                    },
                    size: 65536, // 64KB file
                },
            ]
        }

        fn validate_serialized_data(&self, data: &[u8]) -> Result<()> {
            if data.len() != 18 {
                return Err(StorageError::InvalidFormat(format!(
                    "IndexEntry should be 18 bytes, got {}",
                    data.len()
                )));
            }

            // Validate key bytes (first 9 bytes)
            if data[0..9] != self.key {
                return Err(StorageError::InvalidFormat(
                    "Key bytes mismatch in serialized data".to_string(),
                ));
            }

            // Validate size is little-endian (last 4 bytes)
            let size_bytes = &data[14..18];
            let expected_size_bytes = self.size.to_le_bytes(); // Size is little-endian
            if size_bytes != expected_size_bytes {
                return Err(StorageError::InvalidFormat(
                    "Size field not in little-endian format".to_string(),
                ));
            }

            Ok(())
        }
    }

    // Note: ArchiveLocation doesn't implement BinaryFormatValidator because it's not
    // directly serialized - it's embedded within IndexEntry using custom read/write functions
}
