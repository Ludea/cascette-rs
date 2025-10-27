# cascette-client-storage

Local CASC storage implementation for game installations.

## Status

Working implementation with index management, archive handling, and IPC.

## Features

- Local .idx index file management with bucket algorithm
- Archive .data file handling
- Content key resolution and lookup
- Shared memory IPC for Battle.net Agent compatibility
- Storage information and statistics
- Round-trip validation for binary formats

## Storage Structure

```text
Data/
├── indices/
│   └── *.idx     # Index files (9-byte truncated keys)
├── data/
│   └── *.data    # Archive data files
├── config/       # Configuration files
└── shmem/        # Shared memory files
```

## Components

- `index` - Local .idx file management with bucket algorithm
- `archive` - Archive .data file handling
- `shmem` - Shared memory IPC for Battle.net Agent compatibility
- `storage` - Main storage system and content resolution
- `validation` - Round-trip validation for binary formats

## Dependencies

- `memmap2` - Memory-mapped file access
- `thiserror` - Error handling
- `binrw` - Binary format parsing
- `cascette-formats` - CASC format support

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

---

**Note**: This project is not affiliated with Blizzard Entertainment. It is
an independent implementation based on reverse engineering by the World of
Warcraft emulation community.
