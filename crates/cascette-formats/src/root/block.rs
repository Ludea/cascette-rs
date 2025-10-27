//! Root file block structures and parsing logic

use crate::root::{
    entry::{RootRecord, decode_file_data_ids, encode_file_data_ids},
    error::Result,
    flags::{ContentFlags, LocaleFlags},
    version::RootVersion,
};
use binrw::{BinRead, BinWrite};
use cascette_crypto::md5::{ContentKey, FileDataId};
use std::io::{Read, Seek, Write};

/// Block header containing metadata for file entries
/// Note: Blocks use little-endian encoding (unlike headers which use big-endian)
#[derive(BinRead, BinWrite, Debug, Clone, PartialEq, Eq)]
#[brw(little)]
pub struct RootBlockHeader {
    /// Number of file records in this block
    pub num_records: u32,
    /// Content flags for all files in this block
    pub content_flags: u32, // Will be converted to ContentFlags
    /// Locale flags for all files in this block
    pub locale_flags: LocaleFlags,
}

/// Complete root block with header and file records
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootBlock {
    /// Block header
    pub header: RootBlockHeader,
    /// File records in this block
    pub records: Vec<RootRecord>,
}

impl RootBlock {
    /// Create new empty block
    pub fn new(content_flags: ContentFlags, locale_flags: LocaleFlags) -> Self {
        Self {
            header: RootBlockHeader {
                num_records: 0,
                content_flags: (content_flags.value & 0xFFFF_FFFF) as u32,
                locale_flags,
            },
            records: Vec::new(),
        }
    }

    /// Add record to block
    pub fn add_record(&mut self, record: RootRecord) {
        self.records.push(record);
        // Since we're building the file, we can assume the number of records fits in u32
        // CASC root files are not expected to have more than 4 billion records
        #[allow(clippy::cast_possible_truncation)]
        {
            self.header.num_records = self.records.len() as u32;
        }
    }

    /// Get content flags as `ContentFlags`
    pub fn content_flags(&self) -> ContentFlags {
        ContentFlags::new(u64::from(self.header.content_flags))
    }

    /// Get locale flags
    pub const fn locale_flags(&self) -> LocaleFlags {
        self.header.locale_flags
    }

    /// Get number of records
    pub const fn num_records(&self) -> u32 {
        self.header.num_records
    }

    /// Check if block has name hashes
    pub fn has_name_hashes(&self, version: RootVersion, has_named_files: bool) -> bool {
        match version {
            RootVersion::V1 => true, // V1 always has name hashes
            RootVersion::V2 | RootVersion::V3 | RootVersion::V4 => {
                has_named_files && self.content_flags().has_name_hashes()
            }
        }
    }

    /// Parse block from reader based on version
    pub fn parse<R: Read + Seek>(
        reader: &mut R,
        version: RootVersion,
        has_named_files: bool,
    ) -> Result<Self> {
        // Read block header
        let header = RootBlockHeader::read_le(reader)?;

        if header.num_records == 0 {
            return Ok(Self {
                header,
                records: Vec::new(),
            });
        }

        let count = header.num_records as usize;
        let content_flags = ContentFlags::new(u64::from(header.content_flags));

        match version {
            RootVersion::V1 => parse_v1_block(reader, header, count),
            RootVersion::V2 | RootVersion::V3 => {
                parse_v2_v3_block(reader, header, count, has_named_files, content_flags)
            }
            RootVersion::V4 => {
                parse_v4_block(reader, header, count, has_named_files, content_flags)
            }
        }
    }

    /// Write block to writer based on version
    pub fn write<W: Write + Seek>(
        &self,
        writer: &mut W,
        version: RootVersion,
        has_named_files: bool,
    ) -> Result<()> {
        // Write header
        self.header.write_le(writer)?;

        if self.records.is_empty() {
            return Ok(());
        }

        match version {
            RootVersion::V1 => write_v1_block(writer, &self.records),
            RootVersion::V2 | RootVersion::V3 => {
                write_v2_v3_block(writer, &self.records, has_named_files, self.content_flags())
            }
            RootVersion::V4 => {
                write_v4_block(writer, &self.records, has_named_files, self.content_flags())
            }
        }
    }

    /// Sort records by `FileDataID` for optimal delta encoding
    pub fn sort_records(&mut self) {
        self.records.sort_by_key(|r| r.file_data_id);
    }

    /// Calculate block size in bytes for given version
    pub fn calculate_size(&self, version: RootVersion, has_named_files: bool) -> usize {
        let header_size = 12; // num_records(4) + content_flags(4) + locale_flags(4)
        let count = self.records.len();

        if count == 0 {
            return header_size;
        }

        let fdid_size = count * 4; // i32 deltas
        let ckey_size = count * 16; // MD5 hashes

        let name_hash_size = if self.has_name_hashes(version, has_named_files) {
            count * 8 // u64 hashes
        } else {
            0
        };

        header_size + fdid_size + ckey_size + name_hash_size
    }
}

/// Parse V1 block (interleaved format: `FdidDelta` + `CKey` + `NameHash` repeated)
fn parse_v1_block<R: Read + Seek>(
    reader: &mut R,
    header: RootBlockHeader,
    count: usize,
) -> Result<RootBlock> {
    // Read FileDataID deltas first
    let mut deltas = Vec::with_capacity(count);
    for _ in 0..count {
        deltas.push(i32::read_le(reader)?);
    }
    let fdids = decode_file_data_ids(&deltas);

    // Read interleaved content keys and name hashes
    let mut records = Vec::with_capacity(count);
    for fdid in fdids.iter().take(count) {
        let content_key = ContentKey::read_le(reader)?;
        let name_hash = u64::read_le(reader)?;

        records.push(RootRecord::new(*fdid, content_key, Some(name_hash)));
    }

    Ok(RootBlock { header, records })
}

/// Parse V2/V3 block (separated arrays format)
fn parse_v2_v3_block<R: Read + Seek>(
    reader: &mut R,
    header: RootBlockHeader,
    count: usize,
    has_named_files: bool,
    content_flags: ContentFlags,
) -> Result<RootBlock> {
    // Read FileDataID deltas
    let mut deltas = Vec::with_capacity(count);
    for _ in 0..count {
        deltas.push(i32::read_le(reader)?);
    }
    let fdids = decode_file_data_ids(&deltas);

    // Read content keys array
    let mut content_keys = Vec::with_capacity(count);
    for _ in 0..count {
        content_keys.push(ContentKey::read_le(reader)?);
    }

    // Read name hashes if present
    let has_names = has_named_files && content_flags.has_name_hashes();
    let mut name_hashes = vec![None; count];
    if has_names {
        for hash in name_hashes.iter_mut().take(count) {
            *hash = Some(u64::read_le(reader)?);
        }
    }

    // Combine into records - use the minimum length to handle any array mismatches
    let actual_count = std::cmp::min(
        std::cmp::min(fdids.len(), content_keys.len()),
        name_hashes.len(),
    );
    let mut records = Vec::with_capacity(actual_count);
    for i in 0..actual_count {
        records.push(RootRecord::new(fdids[i], content_keys[i], name_hashes[i]));
    }

    Ok(RootBlock { header, records })
}

/// Parse V4 block (same as V2/V3 but supports extended content flags)
fn parse_v4_block<R: Read + Seek>(
    reader: &mut R,
    header: RootBlockHeader,
    count: usize,
    has_named_files: bool,
    content_flags: ContentFlags,
) -> Result<RootBlock> {
    // V4 parsing is identical to V2/V3 for block structure
    // Extended content flags are handled at the block level
    parse_v2_v3_block(reader, header, count, has_named_files, content_flags)
}

/// Write V1 block (interleaved format)
fn write_v1_block<W: Write + Seek>(writer: &mut W, records: &[RootRecord]) -> Result<()> {
    if records.is_empty() {
        return Ok(());
    }

    // Extract and encode FileDataIDs
    let fdids: Vec<FileDataId> = records.iter().map(|r| r.file_data_id).collect();
    let deltas = encode_file_data_ids(&fdids);

    // Write FileDataID deltas
    for delta in deltas {
        delta.write_le(writer)?;
    }

    // Write interleaved content keys and name hashes
    for record in records {
        record.content_key.write_le(writer)?;

        // V1 always has name hashes
        let name_hash = record.name_hash.unwrap_or(0);
        name_hash.write_le(writer)?;
    }

    Ok(())
}

/// Write V2/V3 block (separated arrays)
fn write_v2_v3_block<W: Write + Seek>(
    writer: &mut W,
    records: &[RootRecord],
    has_named_files: bool,
    content_flags: ContentFlags,
) -> Result<()> {
    if records.is_empty() {
        return Ok(());
    }

    // Write FileDataID deltas
    let fdids: Vec<FileDataId> = records.iter().map(|r| r.file_data_id).collect();
    let deltas = encode_file_data_ids(&fdids);
    for delta in deltas {
        delta.write_le(writer)?;
    }

    // Write content keys array
    for record in records {
        record.content_key.write_le(writer)?;
    }

    // Write name hashes if present
    let has_names = has_named_files && content_flags.has_name_hashes();
    if has_names {
        for record in records {
            let name_hash = record.name_hash.unwrap_or(0);
            name_hash.write_le(writer)?;
        }
    }

    Ok(())
}

/// Write V4 block (same as V2/V3)
fn write_v4_block<W: Write + Seek>(
    writer: &mut W,
    records: &[RootRecord],
    has_named_files: bool,
    content_flags: ContentFlags,
) -> Result<()> {
    // V4 writing is identical to V2/V3 for block structure
    write_v2_v3_block(writer, records, has_named_files, content_flags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn create_test_records() -> Vec<RootRecord> {
        vec![
            RootRecord::new(
                FileDataId::new(100),
                ContentKey::from_hex("0123456789abcdef0123456789abcdef")
                    .expect("Operation should succeed"),
                Some(0x1234_567890abcdef),
            ),
            RootRecord::new(
                FileDataId::new(102),
                ContentKey::from_hex("fedcba9876543210fedcba9876543210")
                    .expect("Operation should succeed"),
                Some(0xfedc_ba0987654321),
            ),
        ]
    }

    #[test]
    fn test_block_header_round_trip() {
        let header = RootBlockHeader {
            num_records: 42,
            content_flags: 0x1234_5678,
            locale_flags: LocaleFlags::new(LocaleFlags::ENUS),
        };

        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        header
            .write_le(&mut cursor)
            .expect("Operation should succeed");

        let mut cursor = Cursor::new(&buffer);
        let restored = RootBlockHeader::read_le(&mut cursor).expect("Operation should succeed");

        assert_eq!(header, restored);
        assert_eq!(buffer.len(), 12); // 4 + 4 + 4 bytes
    }

    #[test]
    fn test_v1_block_round_trip() {
        let mut block = RootBlock::new(
            ContentFlags::new(ContentFlags::INSTALL),
            LocaleFlags::new(LocaleFlags::ENUS),
        );

        for record in create_test_records() {
            block.add_record(record);
        }

        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        block
            .write(&mut cursor, RootVersion::V1, true)
            .expect("Operation should succeed");

        let mut cursor = Cursor::new(&buffer);
        let restored =
            RootBlock::parse(&mut cursor, RootVersion::V1, true).expect("Operation should succeed");

        assert_eq!(block, restored);
    }

    #[test]
    fn test_v2_block_round_trip_with_names() {
        let mut block = RootBlock::new(
            ContentFlags::new(ContentFlags::INSTALL),
            LocaleFlags::new(LocaleFlags::ENUS),
        );

        for record in create_test_records() {
            block.add_record(record);
        }

        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        block
            .write(&mut cursor, RootVersion::V2, true)
            .expect("Operation should succeed");

        let mut cursor = Cursor::new(&buffer);
        let restored =
            RootBlock::parse(&mut cursor, RootVersion::V2, true).expect("Operation should succeed");

        assert_eq!(block, restored);
    }

    #[test]
    fn test_v2_block_round_trip_without_names() {
        let mut block = RootBlock::new(
            ContentFlags::new(ContentFlags::INSTALL | ContentFlags::NO_NAME_HASH),
            LocaleFlags::new(LocaleFlags::ENUS),
        );

        // Create records without name hashes
        let records = vec![
            RootRecord::new(
                FileDataId::new(100),
                ContentKey::from_hex("0123456789abcdef0123456789abcdef")
                    .expect("Operation should succeed"),
                None,
            ),
            RootRecord::new(
                FileDataId::new(102),
                ContentKey::from_hex("fedcba9876543210fedcba9876543210")
                    .expect("Operation should succeed"),
                None,
            ),
        ];

        for record in records {
            block.add_record(record);
        }

        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        block
            .write(&mut cursor, RootVersion::V2, true)
            .expect("Operation should succeed");

        let mut cursor = Cursor::new(&buffer);
        let restored =
            RootBlock::parse(&mut cursor, RootVersion::V2, true).expect("Operation should succeed");

        assert_eq!(block, restored);
    }

    #[test]
    fn test_v3_block_round_trip() {
        let mut block = RootBlock::new(
            ContentFlags::new(ContentFlags::INSTALL),
            LocaleFlags::new(LocaleFlags::ENUS | LocaleFlags::DEDE),
        );

        for record in create_test_records() {
            block.add_record(record);
        }

        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        block
            .write(&mut cursor, RootVersion::V3, true)
            .expect("Operation should succeed");

        let mut cursor = Cursor::new(&buffer);
        let restored =
            RootBlock::parse(&mut cursor, RootVersion::V3, true).expect("Operation should succeed");

        assert_eq!(block, restored);
    }

    #[test]
    fn test_v4_block_round_trip() {
        let mut block = RootBlock::new(
            ContentFlags::new(ContentFlags::INSTALL | ContentFlags::BUNDLE),
            LocaleFlags::new(LocaleFlags::ENUS),
        );

        for record in create_test_records() {
            block.add_record(record);
        }

        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        block
            .write(&mut cursor, RootVersion::V4, true)
            .expect("Operation should succeed");

        let mut cursor = Cursor::new(&buffer);
        let restored =
            RootBlock::parse(&mut cursor, RootVersion::V4, true).expect("Operation should succeed");

        assert_eq!(block, restored);
    }

    #[test]
    fn test_empty_block() {
        let block = RootBlock::new(
            ContentFlags::new(ContentFlags::NONE),
            LocaleFlags::new(LocaleFlags::ALL),
        );

        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        block
            .write(&mut cursor, RootVersion::V2, true)
            .expect("Operation should succeed");

        let mut cursor = Cursor::new(&buffer);
        let restored =
            RootBlock::parse(&mut cursor, RootVersion::V2, true).expect("Operation should succeed");

        assert_eq!(block, restored);
        assert_eq!(restored.records.len(), 0);
        assert_eq!(buffer.len(), 12); // Just header
    }

    #[test]
    fn test_block_sort_records() {
        let mut block = RootBlock::new(
            ContentFlags::new(ContentFlags::INSTALL),
            LocaleFlags::new(LocaleFlags::ENUS),
        );

        // Add records in reverse order
        let records = vec![
            RootRecord::new(
                FileDataId::new(300),
                ContentKey::from_hex("0123456789abcdef0123456789abcdef")
                    .expect("Operation should succeed"),
                Some(0x1111_111111111111),
            ),
            RootRecord::new(
                FileDataId::new(100),
                ContentKey::from_hex("fedcba9876543210fedcba9876543210")
                    .expect("Operation should succeed"),
                Some(0x2222_222222222222),
            ),
            RootRecord::new(
                FileDataId::new(200),
                ContentKey::from_hex("abcdefabcdefabcdefabcdefabcdefab")
                    .expect("Operation should succeed"),
                Some(0x3333_333333333333),
            ),
        ];

        for record in records {
            block.add_record(record);
        }

        // Should be unsorted
        assert_eq!(block.records[0].file_data_id, FileDataId::new(300));
        assert_eq!(block.records[1].file_data_id, FileDataId::new(100));
        assert_eq!(block.records[2].file_data_id, FileDataId::new(200));

        block.sort_records();

        // Should now be sorted
        assert_eq!(block.records[0].file_data_id, FileDataId::new(100));
        assert_eq!(block.records[1].file_data_id, FileDataId::new(200));
        assert_eq!(block.records[2].file_data_id, FileDataId::new(300));
    }

    #[test]
    fn test_block_size_calculation() {
        let mut block = RootBlock::new(
            ContentFlags::new(ContentFlags::INSTALL),
            LocaleFlags::new(LocaleFlags::ENUS),
        );

        // Empty block
        assert_eq!(block.calculate_size(RootVersion::V2, true), 12); // Just header

        // Add records
        for record in create_test_records() {
            block.add_record(record);
        }

        // V2 with names: header(12) + fdids(8) + ckeys(32) + names(16) = 68
        assert_eq!(block.calculate_size(RootVersion::V2, true), 68);

        // V2 without names: header(12) + fdids(8) + ckeys(32) = 52
        let mut no_names_block = RootBlock::new(
            ContentFlags::new(ContentFlags::INSTALL | ContentFlags::NO_NAME_HASH),
            LocaleFlags::new(LocaleFlags::ENUS),
        );
        for record in create_test_records() {
            no_names_block.add_record(RootRecord::new(
                record.file_data_id,
                record.content_key,
                None,
            ));
        }
        assert_eq!(no_names_block.calculate_size(RootVersion::V2, true), 52);
    }
}
