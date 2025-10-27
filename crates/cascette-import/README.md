# cascette-import

Community data source integration for NGDP/CASC operations.

## Status

Working implementation with TACT key import, build history (wago.tools), and
file ID mappings (WoWDev listfile).

## Features

- TACT encryption key import from GitHub repositories (19,000+ keys)
- Build history integration with wago.tools API
- File ID to path mappings from WoWDev listfile
- Unified provider interface for multiple data sources
- Automatic updates and deduplication

## Components

- `tactkeys` - TACT encryption key import from GitHub repositories
- `wago` - wago.tools build information API integration
- `listfile` - WoWDev file ID to path mappings

## Data Sources

- TACT keys: github.com/wowdev/TACTKeys
- Build history: wago.tools API
- File listings: github.com/wowdev/wow-listfile

## Available Features

- `default` - All data sources (wago, listfile, tactkeys)
- `wago` - wago.tools integration
- `listfile` - WoWDev listfile integration
- `tactkeys` - TACT key repository integration
- `p2p` - Peer-to-peer content distribution (future)
- `custom-cdn` - Custom CDN endpoints (future)

## Dependencies

- `reqwest` - HTTP client for API access
- `serde` - JSON deserialization
- `tokio` - Async runtime
- `git2` - Git repository operations
- `cascette-crypto` - Cryptographic operations

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
