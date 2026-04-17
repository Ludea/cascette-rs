use serde::Deserialize;
use serde_json::{Value, json};

/// Query parameters for GET /version.
#[derive(Debug, Deserialize)]
pub struct VersionQuery {
    /// Product identifier (optional, accepted for wire compatibility, ignored).
    pub uid: Option<String>,
}

pub async fn version() -> Value {
    let pkg_version = env!("CARGO_PKG_VERSION");

    json!({
        "agent_version": format!("cascette-agent {pkg_version}"),
        "product_version": pkg_version,
        "build_date": env!("CASCETTE_BUILD_DATE"),
    })
}
