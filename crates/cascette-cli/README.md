# cascette-cli

Command-line interface for NGDP product installation and management.

## Status

Working implementation with product discovery, installation execution
(Battle.net mode), cache management, TACT key management, and FileDataID
resolution.

## Commands

### Product Discovery

```bash
cascette list                    # List available products
cascette info <product>          # Query product information
```

### Product Management

```bash
cascette install <product> <path>    # Install with Battle.net structure
  --plan-only                        # Create plan without downloading
  --execute-plan <path>              # Execute existing plan
  --dry-run                          # Show what would be downloaded
  --simple                           # Extract without Battle.net structure
cascette upgrade                     # Update cascette to latest version
cascette verify                      # Verify product integrity
```

### Data Management

```bash
cascette cache stats             # Show cache statistics
cascette cache clear             # Clear cache contents

cascette config show             # Display configuration
cascette config set              # Set configuration values

cascette tact list               # List imported keys
cascette tact import             # Import keys from sources

cascette fdid browse             # Browse FileDataID mappings
cascette fdid lookup             # Look up file by ID or path
cascette fdid stats              # Show mapping statistics
```

### Utilities

```bash
cascette import                  # Import community data sources
cascette paths                   # Show data storage locations
cascette version                 # Show version information
```

## Configuration

Configuration stored in platform-specific directories:

- Linux: `~/.config/cascette/`
- macOS: `~/Library/Application Support/cascette/`
- Windows: `%APPDATA%\cascette\`

## Dependencies

- `clap` - Command-line parsing
- `tokio` - Async runtime
- `indicatif` - Progress bars
- `cascette-protocol` - NGDP network client
- `cascette-formats` - Binary format support
- `cascette-client-storage` - Local CASC storage
- `cascette-metadata` - Content metadata orchestration

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
