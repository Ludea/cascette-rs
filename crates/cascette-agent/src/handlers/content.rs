use std::path::PathBuf;
use std::sync::Arc;

use crate::handlers::AppState;

/// GET /content/{hash} -- serve content by encoding key hash.
pub async fn content(state: &Arc<AppState>, hash: String) -> Result<Vec<u8>, String> {
    let Ok(encoding_key) = cascette_crypto::EncodingKey::from_hex(&hash) else {
        return Err("invalid encoding key hex: {hash}".to_string());
    };

    // Collect install paths from all registered products.
    let products = match state.registry.list().await {
        Ok(p) => p,
        Err(e) => return Err(format!("registry error: {e}")),
    };

    let install_paths: Vec<PathBuf> = products
        .iter()
        .filter_map(|p| p.install_path.as_ref().map(PathBuf::from))
        .filter(|p| p.exists())
        .collect();

    if install_paths.is_empty() {
        return Err("no installations registered".to_string());
    }

    // Search each installation for the encoding key.
    for path in &install_paths {
        let Ok(installation) = cascette_client_storage::Installation::open(path.join("Data"))
        else {
            continue;
        };

        if let Err(_e) = installation.initialize().await {
            continue;
        }

        if !installation.has_encoding_key(&encoding_key).await {
            continue;
        }

        if let Ok(data) = installation.read_file_by_encoding_key(&encoding_key).await {
            return Ok(data);
        }
    }
    return Err("encoding key not found: {hash}".to_string());
}
