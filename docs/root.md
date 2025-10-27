# Root File Format

The Root file is the primary catalog of all files stored in CASC archives. It
maps file paths or FileDataIDs to content keys, enabling game clients to locate
and retrieve specific assets.

## Overview

The Root file serves as the master index for all game content:

- Maps FileDataIDs to content keys

- Supports multiple locales and content flags

- Groups files into blocks for efficient lookup

- Handles both named and unnamed entries

## File Structure

The Root file is BLTE-encoded and organized into blocks:

```text
[BLTE Container]
  [Header]
  [Block 1]
  [Block 2]
  ...
  [Block N]
```

## Binary Format

### Version Detection

The Root file format has evolved significantly:

- **Pre-30080**: No MFST magic, raw block data

- **Build 30080+ (v2)**: MFST magic with file counts

- **Build 50893+ (v3)**: Added header_size/version fields

- **Build 58221+ (v4)**: Extended content flags to 40 bits

### Header Structures

#### Version 2 (Build 30080+)

```c
struct RootHeaderV2 {
    uint32_t magic;              // 'MFST' (0x4D465354) or 'TSFM' (0x5453464D)
    uint32_t total_file_count;   // Total number of files
    uint32_t named_file_count;   // Number of named entries
};
```

**Note**: Some builds use 'TSFM' magic instead of 'MFST'. This appears to be
a little-endian representation. Both should be accepted as valid.

#### Version 3 (Build 50893+)

```c
struct RootHeaderV3 {
    uint32_t magic;              // 'MFST' (0x4D465354) or 'TSFM' (0x5453464D)
    uint32_t header_size;        // Size of header (20 bytes)
    uint32_t version;            // Version (1)
    uint32_t total_file_count;   // Total number of files
    uint32_t named_file_count;   // Number of named entries
    uint32_t padding;            // Padding (0)
};
```

**Note**: Version 3 also uses TSFM magic in observed builds, maintaining
consistency with Version 2.

**Version Detection Heuristic**: If total_file_count < 1000 after reading
8 bytes past MFST, it's likely v3+ (rewind and read header_size/version).

### Block Structure

Each block contains file entries for specific locale and content flag
combinations:

#### Standard Block (Build 30080+)

```c
struct RootBlock {
    uint32_t num_records;        // Number of records in block
    uint32_t content_flags;      // Content flags (32-bit)
    uint32_t locale_flags;       // Locale flags (language/region)

    // FileDataID deltas (delta-encoded)
    int32_t fileDataIDDeltas[num_records];

    // Record data (format varies by version)
    RootRecord records[num_records];
};
```

#### Extended Block (Build 58221+)

```c
#pragma pack(push, 1)
struct RootBlockV4 {
    uint32_t num_records;        // Number of records in block
    uint32_t locale_flags;       // Locale flags (language/region)
    uint32_t content_flags1;     // Content flags part 1 (32-bit)
    uint32_t content_flags2;     // Content flags part 2 (32-bit)
    uint8_t  content_flags3;     // Content flags part 3 (8-bit)

    // FileDataID deltas and records follow
};
#pragma pack(pop)
```

Note: The 40-bit content flags in v4 require careful alignment handling.

### Record Formats

#### Old Format (Interleaved)

```c
struct RootRecordOld {
    uint8_t content_key[16];     // MD5 content key
    uint8_t name_hash[8];        // Jenkins96 name hash (optional)
};
```

#### New Format (Separated)

```c
struct RootRecordNew {
    // Arrays stored separately
    uint8_t content_keys[num_records][16];
    uint8_t name_hashes[num_records][8];  // Optional
};
```

## Content Flags

Content flags specify platform, architecture, and file attributes:

### 32-bit Flags (v2-v3)

| Bit | Flag | Description |
|-----|------|-------------|
| 0 | LoadOnWindows | Windows platform |
| 1 | LoadOnMacOS | macOS platform |
| 3 | LowViolence | Censored content |
| 9 | DoNotLoad | Skip file |
| 10 | UpdatePlugin | Launcher plugin |
| 11 | Arm64 | ARM64 architecture |
| 12 | Encrypted | Encrypted content |
| 13 | NoNameHash | No name hash in block |
| 14 | UncommonResolution | Non-standard resolution |
| 15 | Bundle | Bundled content |
| 16 | NoCompression | Uncompressed |
| 17 | NoTOCHash | No table of contents hash |

### 40-bit Flags (v4+)

Build 58221+ extends to 40 bits:

- Bits 0-31: ContentFlags1 (as above)

- Bits 32-39: ContentFlags2 (additional flags)

- Bit 40: ContentFlags3 (single byte extension)

Common combinations:

- `0x00000000`: All platforms, default

- `0x00000001`: Windows only

- `0x00000002`: macOS only

- `0x00001000`: Encrypted content

- `0x00002000`: No name hash present

## Locale Flags

32-bit field representing language/region:

| Value | Locale | Description |
|-------|--------|-------------|
| 0x00000002 | enUS | English (US) |
| 0x00000004 | koKR | Korean |
| 0x00000010 | frFR | French |
| 0x00000020 | deDE | German |
| 0x00000040 | zhCN | Chinese (Simplified) |
| 0x00000080 | esES | Spanish (Spain) |
| 0x00000100 | zhTW | Chinese (Traditional) |
| 0x00000200 | enGB | English (UK) |
| 0x00000400 | enCN | English (China) |
| 0x00000800 | enTW | English (Taiwan) |
| 0x00001000 | esMX | Spanish (Mexico) |
| 0x00002000 | ruRU | Russian |
| 0x00004000 | ptBR | Portuguese (Brazil) |
| 0x00008000 | itIT | Italian |
| 0x00010000 | ptPT | Portuguese (Portugal) |
| 0xFFFFFFFF | All | All locales |

## FileDataID Delta Encoding

FileDataIDs use delta encoding for compression:

```rust
fn decode_file_data_ids(deltas: &[i32]) -> Vec<u32> {
    let mut ids = Vec::new();
    let mut current_id = 0u32;

    for (i, &delta) in deltas.iter().enumerate() {
        if i == 0 {
            // First entry: direct value, not a delta
            current_id = delta as u32;
        } else {
            // Subsequent entries: add delta to previous ID
            current_id = (current_id as i32 + delta) as u32;
        }
        ids.push(current_id);

        // Important: Increment for next iteration
        current_id += 1;
    }

    ids
}
```

**Note**: The algorithm increments current_id by 1 after each entry,
then applies the next delta. This handles sequential FileDataIDs efficiently.

## Lookup Process

1. **Parse Root file**: Decompress BLTE, read header and blocks
2. **Filter by flags**: Select blocks matching desired locale/content
3. **Find FileDataID**: Binary search or iterate through blocks
4. **Extract content key**: Retrieve corresponding MD5 hash
5. **Resolve via encoding**: Use content key to find encoding key

## Name Hash Calculation

For named files, Jenkins96 hash (hashlittle2) is used:

```rust
fn jenkins96_hash(filename: &str) -> u64 {
    // Normalize path: uppercase and forward slashes to backslashes
    let normalized = filename.to_uppercase().replace('/', '\\');
    let bytes = normalized.as_bytes();

    // Jenkins hashlittle2 with 0xDEADBEEF seed
    // Initial values: pc = 0, pb = 0 (passed by reference)
    let (pc, pb) = hashlittle2(bytes, 0, 0);

    // Combine into 64-bit hash (pc is high 32 bits)
    ((pc as u64) << 32) | (pb as u64)
}
```

**Important Jenkins96 Details**:

- Paths are normalized to uppercase with backslashes

- The hash is 64-bit (8 bytes) not 96-bit despite the name

- Some blocks have `NoNameHash` flag, omitting name hashes entirely

- Uses Bob Jenkins' lookup3.c algorithm (hashlittle2 function)

- Processes data in 12-byte chunks with little-endian byte order

- The 0xDEADBEEF constant is added during initialization

- Python validation tool available: `tools/examine_jenkins96.py`

**Example Hashes**:

- Empty string: `0xDEADBEEFDEADBEEF`

- `Interface\Icons\INV_Misc_QuestionMark.blp`: `0x9EB59E3C76124837`

## Implementation Example

```rust
struct RootFile {
    header: RootHeader,
    blocks: Vec<RootBlock>,
}

impl RootFile {
    pub fn find_file(&self, file_data_id: u32) -> Option<MD5Hash> {
        for block in &self.blocks {
            // Check if block matches desired flags
            if !self.matches_flags(block) {
                continue;
            }

            // Search for FileDataID
            if let Some(idx) = block.find_file_index(file_data_id) {
                return Some(block.records[idx].content_key);
            }
        }
        None
    }
}
```

## Version History

- **Build 18125 (6.0.1)**: Initial CASC Root format

- **Build 30080 (8.2.0)**: Added MFST magic signature

- **Build 50893 (10.1.7)**: Added header_size/version fields (v3)

- **Build 58221 (11.1.0)**: Extended content flags to 40 bits (v4)

### Version Detection Code

```rust
fn detect_root_version(data: &[u8]) -> RootVersion {
    if data.len() < 4 {
        return RootVersion::Invalid;
    }

    // Check for MFST or TSFM magic
    let magic = &data[0..4];
    if magic != b"MFST" && magic != b"TSFM" {
        return RootVersion::V1; // Pre-30080, no magic
    }

    // Read potential file counts
    let count1 = u32::from_le_bytes(data[4..8].try_into().unwrap());
    let count2 = u32::from_le_bytes(data[8..12].try_into().unwrap());

    // Heuristic: if first value < 1000, likely v3+ with header_size
    if count1 < 1000 {
        RootVersion::V3 // 50893+
    } else {
        RootVersion::V2 // 30080+
    }
}
```

## Parser Implementation Status

The Python parser (`tools/examine_root.py`) currently supports:

- Version detection (MFST/TSFM magic)

- Version 1-3 parsing

- Block-based extraction

- Content key retrieval

- Delta encoding detection (identifies but doesn't decode)

The parser can extract FileDataID to content key mappings from all current
WoW root file versions.

## Common Issues

1. **Multiple matches**: Same file may exist in multiple blocks with different
locales
2. **Missing entries**: Not all FileDataIDs have corresponding entries
3. **Flag interpretation**: Game-specific flag meanings vary
4. **Delta overflow**: Large gaps in FileDataIDs can cause integer overflow

## References

- See [Encoding Documentation](encoding.md) for content key resolution

- See [BLTE Format](blte.md) for container structure

- See [CDN Architecture](cdn.md) for file retrieval
