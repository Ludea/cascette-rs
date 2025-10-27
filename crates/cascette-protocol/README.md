# cascette-protocol

NGDP/CASC network protocol implementation.

## Status

Working implementation with live Blizzard server support.

## Features

- Ribbit TCP protocol client for service discovery
- TACT HTTP/HTTPS protocol clients for configuration retrieval
- CDN content delivery with range requests and resume support
- Automatic three-tier protocol fallback (HTTPS → HTTP → TCP)
- V1 MIME format with PKCS#7 signature verification
- Retry policies with exponential backoff
- Protocol-specific caching wrapper

## Protocol Fallback

Automatically tries protocols in order:

1. TACT HTTPS (us.version.battle.net)
2. TACT HTTP (us.patch.battle.net:1119)
3. Ribbit TCP (us.version.battle.net:1119)

## Components

- `client/ribbit` - Ribbit TCP protocol client
- `client/tact` - TACT HTTP/HTTPS protocol clients
- `cdn` - CDN content delivery with range requests
- `v1_mime` - V1 MIME format with PKCS#7 signature verification
- `transport` - HTTP and TCP transport implementations
- `retry` - Retry policies with exponential backoff
- `cache` - Protocol-specific caching wrapper

## Dependencies

- `tokio` - Async runtime
- `reqwest` - HTTP client
- `thiserror` - Error handling
- `cascette-cache` - Caching layer
- `cascette-formats` - Binary format parsing

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
