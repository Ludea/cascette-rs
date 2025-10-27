# cascette-rs

Rust implementation of NGDP (Next Generation Distribution Pipeline) and CASC
(Content Addressable Storage Container) for World of Warcraft.

<div align="center">

[![Discord](https://img.shields.io/discord/1394228766414471219?logo=discord&style=flat-square)](https://discord.gg/Q44pPMvGEd)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE-APACHE)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE-MIT)

</div>

## Project Status

cascette-rs implements Blizzard's NGDP/CASC system with core components
implemented and tested.

### Core Components

**Major Components Implemented:**

- **cascette-formats** - Binary format parsers and builders for NGDP/CASC
formats
- **cascette-crypto** - Cryptographic operations (MD5, Jenkins96,
  Salsa20, ARC4, TACT keys)
- **cascette-protocol** - NGDP protocol client with automatic fallback
- **cascette-cache** - Multi-layer caching system
- **cascette-client-storage** - Local CASC storage with shared memory IPC

**Quality Assurance:**

- unit tests across all components
- Integration tests for cross-component validation
- Code quality enforcement with Clippy
- Round-trip validation for binary formats
- Tested with actual WoW installations

### Application Development

Core infrastructure supports user-facing applications:

**Applications:**

- **cascette-cli** - Command-line interface (in development)
- **cascette-agent** - Background service (planned)
- **cascette-launcher** - GUI application (planned)

## Documentation

### [NGDP/CASC Documentation](docs/README.md)

Specifications for NGDP/CASC components:

- **Core Formats**
  - [BLTE Format](docs/blte.md) - Block Table Encoded compression (N, Z, E, F
modes)
  - [Encoding](docs/encoding.md) - Content-to-encoding key mapping
  - [Root](docs/root.md) - File manifest (versions 1-4)
  - [Install](docs/install.md) - Installation manifest and tags
  - [Download](docs/download.md) - Download priority and platform tags
  - [TVFS](docs/tvfs.md) - Virtual file system format
- **Service Discovery**
  - [Ribbit Protocol](docs/ribbit.md) - TCP/HTTP/HTTPS discovery API
  - [BPSV Format](docs/bpsv.md) - Blizzard Pipe-Separated Values
- **Content Delivery**
  - [CDN Architecture](docs/cdn.md) - Content delivery network structure
  - [Configuration Formats](docs/config-formats.md) - Build, CDN,
    Product, Patch configs

  - [Patches](docs/patches.md) - Patch system and ZBSDIFF1 format
  - [ESpec](docs/espec.md) - Encoding specification for patches
- **Security & Distribution**
  - [Salsa20](docs/salsa20.md) - Stream cipher encryption
  - [Mirroring](docs/mirroring.md) - Content mirroring methods
- **Analysis & Evolution**
  - [Format Transitions](docs/format-transitions.md) - Format changes
    by version

## Related Projects

### [cascette-py](https://github.com/wowemulation-dev/cascette-py)

Python development and learning playground for NGDP/CASC:

- Format analysis and prototyping tools
- BLTE decompression implementation
- Wago.tools integration (1,900+ WoW builds database)
- Format examination and verification utilities
- Reference implementations for format parsing

The Python project serves as a prototyping environment for understanding
NGDP/CASC formats before implementing them in Rust.

## Development

### Prerequisites

- Rust 1.86+ (MSRV)
- Rust 2024 edition

### Building

```bash
# Clone the repository
git clone https://github.com/wowemulation-dev/cascette-rs.git
cd cascette-rs

# Build workspace
cargo build --workspace

# Run tests
cargo test --workspace

# Check code quality
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

### Analysis Tools Setup

```bash
# Navigate to tools directory
cd tools

# Fetch build database (requirements.txt not needed - only uses
# requests)
python fetch_wago_builds.py

# Verify tools are working
python test_all_tools.py

# Run verification
python run_format_verification.py
```

### Contributing

- [CONTRIBUTING.md](CONTRIBUTING.md) - Contribution process
- [CONTRIBUTORS.md](CONTRIBUTORS.md) - Contributors list

## License

This project is dual-licensed under either:

- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

---

**Note**: This project is not affiliated with Blizzard Entertainment. It is
an independent implementation based on reverse engineering by the World of
Warcraft emulation community.
