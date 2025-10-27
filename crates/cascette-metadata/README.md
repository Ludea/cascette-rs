# cascette-metadata

Content metadata orchestration layer for NGDP/CASC pipeline.

## Status

Working implementation with build metadata caching and FileDataID resolution.

## Features

- FileDataID to file path mapping with fast lookups
- TACT encryption key management and lookup
- Build metadata caching
- Content category classification
- Validation and health checks
- Statistics tracking

## Components

- `fdid` - FileDataID to file path mapping service
  - Memory-based provider for fast lookups
  - Listfile adapter for community data integration
  - Statistics and browsing capabilities
- `tact` - TACT encryption key management
  - Key lookup by numeric ID
  - Key storage and retrieval
  - Import from community sources
- `orchestrator` - Content metadata orchestration
  - Build metadata caching
  - Content category classification
  - Validation and health checks
  - Statistics tracking

## Available Features

- `default` - Standard functionality
- `import` - Community data integration (requires cascette-import)

## Architecture

This crate sits between the application layer (CLI, launcher) and the
implementation layers (crypto, formats), providing orchestration without
handling the actual cryptographic operations or format parsing.

```text
Applications (CLI, Launcher)
         ↓
   cascette-metadata (orchestration)
         ↓
cascette-crypto + cascette-formats (implementation)
```

## Dependencies

- `tokio` - Async runtime
- `serde` - Serialization
- `chrono` - Date and time handling
- `cascette-crypto` - Cryptographic operations
- `cascette-import` - Community data integration (optional)

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
