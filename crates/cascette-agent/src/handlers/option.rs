use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::handlers::AppState;
use crate::handlers::error_codes::AGENT_ERROR_INVALID_REQUEST;

/// Option request body (POST).
#[derive(Debug, Deserialize)]
pub struct OptionRequest {
    /// Product unique identifier.
    pub uid: Option<String>,
    /// Language setting.
    pub language: Option<String>,
    /// Region setting.
    pub region: Option<String>,
}

/// Query parameters for GET /option.
#[derive(Debug, Deserialize)]
pub struct OptionQuery {
    /// Product unique identifier for per-product option lookup.
    pub uid: Option<String>,
}

pub async fn option(state: &Arc<AppState>, query: OptionQuery) -> Value {
    if let Some(ref uid) = query.uid
        && let Ok(product) = state.registry.get(uid).await
    {
        json!({
            "uid": uid,
            "language": product.locale.as_deref().unwrap_or(&state.config.locale),
            "region": product.region,
        });
    }

    json!({
        "options": {
            "default_locale": state.config.locale,
        },
    })
}

/// POST /option -- update options for a product.
///
/// When a product uid is provided, updates that product's locale/region
/// in the registry. Returns error 2312 if uid is given but product is not found
/// (matches Agent.exe behavior for unknown UIDs).
pub async fn set_option(state: &Arc<AppState>, body: OptionRequest) -> Result<Value, u32> {
    if let Some(ref uid) = body.uid {
        match state.registry.get(uid).await {
            Ok(mut product) => {
                if let Some(ref language) = body.language {
                    product.locale = Some(language.clone());
                }
                if let Some(ref region) = body.region {
                    product.region = Some(region.clone());
                }
                let _ = state.registry.update(&product).await;
            }
            Err(_) => {
                return Err(AGENT_ERROR_INVALID_REQUEST);
            }
        }
    }

    Ok(json!({
        "uid": body.uid,
        "language": body.language,
        "region": body.region,
    }))
}
