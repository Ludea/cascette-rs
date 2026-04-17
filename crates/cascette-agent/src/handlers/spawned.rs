use serde::Deserialize;
use serde_json::{Value, json};

/// POST /spawned or POST /spawned/{product} request body.
///
/// Agent.exe records binary path and launch arguments. Fields are accepted
/// but not stored.
#[derive(Debug, Deserialize)]
pub struct SpawnedRequest {
    /// Product UID. Optional — may also be provided via path parameter.
    #[serde(default)]
    pub uid: String,
}

pub async fn spawned() -> Value {
    json!({"spawned": []})
}

/// GET /spawned/{product} -- per-product spawn status.
pub async fn spawned_product(product: String) -> Value {
    json!({"uid": product, "spawned": false})
}

/// POST /spawned -- record a global spawn event.
///
/// Returns `response_uri`.
pub async fn set_spawned(body: SpawnedRequest) -> Value {
    let response_uri = if body.uid.is_empty() {
        "/spawned".to_string()
    } else {
        format!("/spawned/{}", body.uid)
    };

    json!({"response_uri": response_uri})
}

/// POST /spawned/{product} -- record a per-product spawn event.
///
/// Returns `response_uri` for the spawned product sub-endpoint.
pub async fn set_spawned_product(product: String) -> Value {
    let response_uri = format!("/spawned/{product}");

    json!({"response_uri": response_uri})
}
