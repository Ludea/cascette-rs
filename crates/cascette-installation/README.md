# cascette-installation

Installation library for managing NGDP/CASC product installations, updates, and verification.

## Purpose

This library provides the core installation logic extracted from `cascette-cli`, enabling reuse across different contexts:

- **CLI**: Interactive installation via `cascette install`
- **Agent Service**: Background HTTP service for managing installations
- **Automation**: Scriptable installation for CI/CD pipelines

## Features

- **Installation Planning**: Generate installation plans from NGDP manifests
- **Download Orchestration**: Manage concurrent downloads with progress tracking
- **Resume Support**: Resume interrupted installations from checkpoints
- **Verification**: Validate file integrity against manifests
- **Battle.net Compatibility**: Generate all required metadata files

## Architecture

The library is organized into modules:

- `plan`: Installation planning and manifest resolution
- `executor`: Installation execution engine
- `progress`: Progress tracking and reporting
- `resume`: Checkpoint and resume functionality
- `battlenet`: Battle.net-specific metadata generation
- `metadata`: Metadata file generation
- `models`: Core data structures
- `error`: Error types

## Usage

```rust
use cascette_installation::{plan, executor};

// Create installation plan
let plan = plan::create_installation_plan(product_code, version, path).await?;

// Execute installation
let executor = executor::InstallationExecutor::new(plan);
executor.execute().await?;
```

## Dependencies

- `cascette-formats`: Binary format handling
- `cascette-protocol`: NGDP/Ribbit protocol
- `cascette-cache`: Download caching
- `cascette-client-storage`: Local storage management
- `cascette-metadata`: Metadata operations
- `cascette-crypto`: Cryptographic verification

## License

MIT OR Apache-2.0
