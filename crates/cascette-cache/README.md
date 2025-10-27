# cascette-cache

Multi-layer caching infrastructure for NGDP/CASC content.

## Status

Working implementation with memory and disk caching layers.

## Features

- L1 memory cache with LRU eviction
- L2 disk cache for persistent storage
- NGDP-specific memory pooling with size classes
- TTL-based expiration policies
- Atomic metrics for hit rates and performance tracking
- Type-safe cache keys for NGDP content
- Configurable size limits and eviction policies

## Components

- `layer` - L1 memory and L2 disk cache implementations
- `memory_pool` - NGDP-specific memory pooling with size classes
- `key` - Type-safe cache keys for NGDP content
- `config` - Cache configuration and eviction policies
- `metrics` - Atomic metrics for hit rates and performance
- `ngdp` - NGDP protocol-specific cache wrapper

## Dependencies

- `tokio` - Async runtime
- `serde` - Serialization
- `bincode` - Binary encoding
- `lru` - LRU eviction
- `dashmap` - Concurrent hashmap

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
