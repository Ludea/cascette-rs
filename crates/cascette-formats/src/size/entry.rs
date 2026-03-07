//! Size manifest entry with null-terminated key, key_hash, and variable esize
//!
//! Binary layout per entry:
//!
//! ```text
//! [key bytes: ekey_size bytes]  null-terminated key string
//! [0x00]                        null terminator (part of the key string)
//! [key_hash: 2 bytes BE]        validated: 0x0000 and 0xFFFF are reserved
//! [esize: esize_bytes bytes BE] estimated file size (variable width)
//! ```
//!
//! The total on-disk stride per entry is: `ekey_size + 1 + 2 + esize_bytes`
//! (key + null + hash + esize).

use crate::size::error::{Result, SizeError};
use crate::size::header::SizeHeader;
use binrw::{BinResult, BinWrite};
use std::io::{Read, Seek, Write};

/// A single entry in the Size manifest
///
/// Each entry maps an encoding key to an estimated file size.
///
/// The `key` field contains the raw key bytes (without the null terminator
/// that is present on disk). The `key_hash` field is a 16-bit identifier
/// used for hash map bucketing; values 0x0000 and 0xFFFF are reserved and
/// invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeEntry {
    /// Encoding key bytes (length = `header.ekey_size()` bytes)
    pub key: Vec<u8>,
    /// 16-bit key hash for hash map lookup (must not be 0x0000 or 0xFFFF)
    ///
    /// Stored as big-endian at bytes `[key.len()+1 .. key.len()+3]` on disk.
    pub key_hash: u16,
    /// Estimated file size (variable width, stored as u64)
    pub esize: u64,
}

impl SizeEntry {
    /// Create a new size entry
    pub fn new(key: Vec<u8>, key_hash: u16, esize: u64) -> Self {
        Self {
            key,
            key_hash,
            esize,
        }
    }

    /// Validate this entry's key_hash against the reserved sentinel values.
    ///
    /// 0x0000 and 0xFFFF are reserved and rejected with error code 8.
    pub fn validate_hash(&self) -> Result<()> {
        if self.key_hash == 0x0000 || self.key_hash == 0xFFFF {
            return Err(SizeError::InvalidKeyHash(self.key_hash));
        }
        Ok(())
    }

    /// Validate this entry against the header
    pub fn validate(&self, header: &SizeHeader) -> Result<()> {
        let expected_len = header.ekey_size() as usize;
        if self.key.len() != expected_len {
            return Err(SizeError::TruncatedData {
                expected: expected_len,
                actual: self.key.len(),
            });
        }
        self.validate_hash()?;
        Ok(())
    }

    /// Calculate the on-disk serialized size of an entry for the given header
    ///
    /// Layout: key(ekey_size) + null(1) + key_hash(2) + esize(esize_bytes)
    pub fn serialized_size(header: &SizeHeader) -> usize {
        header.ekey_size() as usize + 1 + 2 + header.esize_bytes() as usize
    }

    /// Read a single entry from the given reader using the header for sizing
    ///
    /// This is the primary deserialization method matching the binary format.
    pub fn read_entry<R: Read>(reader: &mut R, header: &SizeHeader) -> Result<Self> {
        let key_len = header.ekey_size() as usize;

        // Read exactly key_len key bytes
        let mut key = vec![0u8; key_len];
        reader.read_exact(&mut key)?;

        // Read and discard null terminator
        let mut null_buf = [0u8; 1];
        reader.read_exact(&mut null_buf)?;

        // Read 2-byte big-endian key_hash and validate
        let mut hash_buf = [0u8; 2];
        reader.read_exact(&mut hash_buf)?;
        let key_hash = u16::from_be_bytes(hash_buf);
        if key_hash == 0x0000 || key_hash == 0xFFFF {
            return Err(SizeError::InvalidKeyHash(key_hash));
        }

        // Read esize (variable width, big-endian, zero-extend to u64)
        let esize_bytes = header.esize_bytes() as usize;
        let mut esize: u64 = 0;
        if esize_bytes > 0 {
            let mut esize_buf = vec![0u8; esize_bytes];
            reader.read_exact(&mut esize_buf)?;
            for &b in &esize_buf {
                esize = (esize << 8) | u64::from(b);
            }
        }

        Ok(Self {
            key,
            key_hash,
            esize,
        })
    }
}

/// `BinWrite` serialises an entry in the correct on-disk format.
///
/// `BinRead` is not implemented via the binrw derive because the null-terminated
/// key format requires context (the key length from the header) that binrw's
/// derive macro cannot express cleanly. Use `SizeEntry::read_entry` instead.
impl BinWrite for SizeEntry {
    type Args<'a> = &'a SizeHeader;

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        _endian: binrw::Endian,
        header: Self::Args<'_>,
    ) -> BinResult<()> {
        // Write key bytes
        writer.write_all(&self.key)?;
        // Write null terminator
        writer.write_all(&[0x00])?;
        // Write key_hash big-endian
        writer.write_all(&self.key_hash.to_be_bytes())?;

        // Write esize (variable width, big-endian)
        let esize_bytes = header.esize_bytes() as usize;
        if esize_bytes > 0 {
            let mut esize_buf = vec![0u8; esize_bytes];
            for i in 0..esize_bytes {
                esize_buf[esize_bytes - 1 - i] = (self.esize >> (i * 8)) as u8;
            }
            writer.write_all(&esize_buf)?;
        }

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use binrw::io::Cursor;

    fn v1_header(esize_bytes: u8) -> SizeHeader {
        // 72 bits = 9 bytes
        SizeHeader::new_v1(72, 1, 0, 0, esize_bytes)
    }

    fn v2_header() -> SizeHeader {
        SizeHeader::new_v2(72, 1, 0, 0)
    }

    fn make_entry_bytes(key: &[u8], key_hash: u16, esize: u64, esize_bytes: u8) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(key);
        data.push(0x00); // null terminator
        data.extend_from_slice(&key_hash.to_be_bytes());
        for i in (0..esize_bytes as usize).rev() {
            data.push((esize >> (i * 8)) as u8);
        }
        data
    }

    #[test]
    fn test_parse_entry_v1_context() {
        let header = v1_header(4);
        let key = vec![0xAB; 9];
        let key_hash: u16 = 0x1234;
        let esize: u32 = 0x0000_5678;

        let data = make_entry_bytes(&key, key_hash, esize as u64, 4);

        let entry =
            SizeEntry::read_entry(&mut data.as_slice(), &header).expect("Should parse entry");

        assert_eq!(entry.key, key);
        assert_eq!(entry.key_hash, 0x1234);
        assert_eq!(entry.esize, 0x5678);
    }

    #[test]
    fn test_parse_entry_v2_context() {
        let header = v2_header();
        let key = vec![0xCD; 9];
        let key_hash: u16 = 0xABCD;
        let esize: u32 = 0x0001_0000;

        let data = make_entry_bytes(&key, key_hash, esize as u64, 4);

        let entry =
            SizeEntry::read_entry(&mut data.as_slice(), &header).expect("Should parse entry");

        assert_eq!(entry.key, key);
        assert_eq!(entry.key_hash, 0xABCD);
        assert_eq!(entry.esize, 0x0001_0000);
    }

    #[test]
    fn test_entry_round_trip() {
        let header = v1_header(4);
        let entry = SizeEntry::new(vec![0x11; 9], 0x5678, 1024);

        let mut buf = Vec::new();
        let mut cursor = Cursor::new(&mut buf);
        entry
            .write_options(&mut cursor, binrw::Endian::Big, &header)
            .expect("Should write entry");

        // key(9) + null(1) + hash(2) + esize(4) = 16
        assert_eq!(buf.len(), 16);

        let parsed =
            SizeEntry::read_entry(&mut buf.as_slice(), &header).expect("Should parse entry");

        assert_eq!(entry, parsed);
    }

    #[test]
    fn test_serialized_size() {
        // V1 with 4-byte esize: 9 + 1 + 2 + 4 = 16
        let header = v1_header(4);
        assert_eq!(SizeEntry::serialized_size(&header), 16);

        // V1 with 1-byte esize: 9 + 1 + 2 + 1 = 13
        let header = v1_header(1);
        assert_eq!(SizeEntry::serialized_size(&header), 13);

        // V2: 9 + 1 + 2 + 4 = 16
        let header = v2_header();
        assert_eq!(SizeEntry::serialized_size(&header), 16);
    }

    #[test]
    fn test_esize_width_1_byte() {
        let header = v1_header(1);
        let entry = SizeEntry::new(vec![0x00; 9], 0x0001, 255);

        let mut buf = Vec::new();
        let mut cursor = Cursor::new(&mut buf);
        entry
            .write_options(&mut cursor, binrw::Endian::Big, &header)
            .expect("Should write entry");

        // key(9) + null(1) + hash(2) + esize(1) = 13
        assert_eq!(buf.len(), 13);

        let parsed =
            SizeEntry::read_entry(&mut buf.as_slice(), &header).expect("Should parse entry");
        assert_eq!(parsed.esize, 255);
    }

    #[test]
    fn test_esize_width_8_bytes() {
        let header = v1_header(8);
        let big_size: u64 = 0x0123_4567_89AB_CDEF;
        let entry = SizeEntry::new(vec![0x00; 9], 0x0002, big_size);

        let mut buf = Vec::new();
        let mut cursor = Cursor::new(&mut buf);
        entry
            .write_options(&mut cursor, binrw::Endian::Big, &header)
            .expect("Should write entry");

        // key(9) + null(1) + hash(2) + esize(8) = 20
        assert_eq!(buf.len(), 20);

        let parsed =
            SizeEntry::read_entry(&mut buf.as_slice(), &header).expect("Should parse entry");
        assert_eq!(parsed.esize, big_size);
    }

    #[test]
    fn test_esize_width_0_bytes() {
        // V1 with esize_bytes=0: entries have only key + null + hash
        let header = v1_header(0);
        let entry = SizeEntry::new(vec![0x00; 9], 0x0042, 0);

        let mut buf = Vec::new();
        let mut cursor = Cursor::new(&mut buf);
        entry
            .write_options(&mut cursor, binrw::Endian::Big, &header)
            .expect("Should write entry");

        // key(9) + null(1) + hash(2) + esize(0) = 12
        assert_eq!(buf.len(), 12);

        let parsed =
            SizeEntry::read_entry(&mut buf.as_slice(), &header).expect("Should parse entry");
        assert_eq!(parsed.esize, 0);
        assert_eq!(parsed.key_hash, 0x0042);
    }

    #[test]
    fn test_reject_reserved_key_hash_0x0000() {
        let header = v1_header(4);
        let data = make_entry_bytes(&vec![0xAA; 9], 0x0000, 100, 4);
        let result = SizeEntry::read_entry(&mut data.as_slice(), &header);
        assert!(matches!(result, Err(SizeError::InvalidKeyHash(0x0000))));
    }

    #[test]
    fn test_reject_reserved_key_hash_0xffff() {
        let header = v1_header(4);
        let data = make_entry_bytes(&vec![0xAA; 9], 0xFFFF, 100, 4);
        let result = SizeEntry::read_entry(&mut data.as_slice(), &header);
        assert!(matches!(result, Err(SizeError::InvalidKeyHash(0xFFFF))));
    }

    #[test]
    fn test_validate_entry() {
        let header = v1_header(4);

        let valid = SizeEntry::new(vec![0x00; 9], 0x0001, 100);
        assert!(valid.validate(&header).is_ok());

        // Wrong key length
        let bad_key_len = SizeEntry::new(vec![0x00; 16], 0x0001, 100);
        assert!(bad_key_len.validate(&header).is_err());

        // Reserved hash
        let bad_hash = SizeEntry::new(vec![0x00; 9], 0x0000, 100);
        assert!(bad_hash.validate(&header).is_err());
    }

    #[test]
    fn test_full_ekey_size() {
        // 128 bits = 16-byte EKey
        let header = SizeHeader::new_v1(128, 1, 0, 42, 4);
        let entry = SizeEntry::new(vec![0xAA; 16], 0x1234, 42);

        let mut buf = Vec::new();
        let mut cursor = Cursor::new(&mut buf);
        entry
            .write_options(&mut cursor, binrw::Endian::Big, &header)
            .expect("Should write entry");

        // key(16) + null(1) + hash(2) + esize(4) = 23
        assert_eq!(buf.len(), 23);

        let parsed =
            SizeEntry::read_entry(&mut buf.as_slice(), &header).expect("Should parse entry");
        assert_eq!(parsed, entry);
    }
}
