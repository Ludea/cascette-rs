//! Simple TACT keys fetcher for CLI integration

use crate::error::{ImportError, ImportResult};
use cascette_crypto::{TactKey, TactKeyStore};
use reqwest::Client;
use std::io::{BufRead, BufReader};
use std::time::Duration;
use tracing::{debug, info};

/// Raw TACT keys download URL
const TACTKEYS_RAW_URL: &str = "https://raw.githubusercontent.com/wowdev/TACTKeys/master/WoW.txt";

/// Fetch TACT keys from GitHub `WoWDev` repository
pub async fn fetch_github_tactkeys() -> ImportResult<TactKeyStore> {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(format!("cascette-rs/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("Failed to create HTTP client");

    debug!("Fetching TACT keys from GitHub");

    let response = client
        .get(TACTKEYS_RAW_URL)
        .send()
        .await
        .map_err(ImportError::Network)?;

    if !response.status().is_success() {
        return Err(ImportError::HttpStatus {
            provider: "github-tactkeys".to_string(),
            status: response.status().as_u16(),
            message: response.text().await.unwrap_or_default(),
        });
    }

    let content = response.text().await.map_err(ImportError::Network)?;
    debug!("Downloaded TACT keys content: {} bytes", content.len());

    parse_tact_keys_content(&content)
}

/// Parse TACT keys file content
/// Check if a line should be skipped (empty or comment)
fn should_skip_line(line: &str) -> bool {
    line.trim().is_empty() || line.starts_with('#') || line.starts_with("//")
}

/// Validate hex string format
fn is_valid_hex(s: &str, expected_len: usize) -> bool {
    s.len() == expected_len && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Parse a single TACT key line and add to store
fn parse_and_add_key(line: &str, line_num: usize, key_store: &TactKeyStore) -> Option<()> {
    let parts: Vec<&str> = line.split_whitespace().collect();

    if parts.len() < 2 {
        debug!("Invalid line format on line {}: {}", line_num, line);
        return None;
    }

    let lookup_hex = parts[0];
    let key_hex = parts[1];

    if !is_valid_hex(lookup_hex, 16) || !is_valid_hex(key_hex, 32) {
        debug!("Invalid key format on line {}: {}", line_num, line);
        return None;
    }

    let key_id = u64::from_str_radix(lookup_hex, 16).ok()?;
    let tact_key = TactKey::from_hex(key_id, key_hex).ok()?;

    key_store.add(tact_key).ok()?;
    Some(())
}

fn parse_tact_keys_content(content: &str) -> ImportResult<TactKeyStore> {
    let key_store = TactKeyStore::new().map_err(|e| ImportError::Provider {
        provider: "github-tactkeys".to_string(),
        message: format!("Failed to create keyring store: {}", e),
    })?;

    let reader = BufReader::new(content.as_bytes());
    let mut line_count = 0;
    let mut parsed_count = 0;

    for line in reader.lines() {
        line_count += 1;
        let line = line.map_err(ImportError::Io)?;

        if should_skip_line(&line) {
            continue;
        }

        if parse_and_add_key(&line, line_count, &key_store).is_some() {
            parsed_count += 1;
        }
    }

    info!(
        "Parsed {} TACT keys from {} lines",
        parsed_count, line_count
    );
    Ok(key_store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tact_keys_parsing() {
        let content = r"# TACT Keys for World of Warcraft
// Comment line
0123456789ABCDEF 0123456789ABCDEF0123456789ABCDEF
FEDCBA9876543210 FEDCBA9876543210FEDCBA9876543210 Additional info

// Invalid lines
INVALID_HEX ABCD1234567890ABCDEF1234567890AB
0123456789ABCDEF INVALID_KEY
";

        let key_store = parse_tact_keys_content(content).expect("test content should be valid");

        assert_eq!(key_store.len(), 2);

        // Test first key (0123456789ABCDEF)
        let first_key_id = 0x0123_4567_89AB_CDEF_u64;
        let first_key = key_store
            .get(first_key_id)
            .expect("Should be able to get key")
            .expect("First key should exist");
        assert_eq!(
            first_key,
            <[u8; 16]>::try_from(
                hex::decode("0123456789ABCDEF0123456789ABCDEF").expect("Valid hex decode")
            )
            .expect("Valid array conversion")
        );

        // Test second key (FEDCBA9876543210)
        let second_key_id = 0xFEDC_BA98_7654_3210_u64;
        let second_key = key_store
            .get(second_key_id)
            .expect("Should be able to get key")
            .expect("Second key should exist");
        assert_eq!(
            second_key,
            <[u8; 16]>::try_from(
                hex::decode("FEDCBA9876543210FEDCBA9876543210").expect("Valid hex decode")
            )
            .expect("Valid array conversion")
        );
    }
}
