//! HTTP API request and response models
//!
//! Provides strongly-typed models for the REST API with proper serialization
//! and validation. Implements HATEOAS principles for discoverability.

use axum::Json;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::models::{
    InstallationMode, Operation, OperationState, OperationType, Product, ProductStatus,
};

/// Install request body
///
/// Request to install a product with specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallRequest {
    /// Product build ID (optional, use latest if not specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_id: Option<u32>,

    /// Installation directory path
    pub install_path: String,

    /// Product region (us, eu, kr, cn, tw)
    pub region: String,

    /// Product locale (enUS, enGB, deDE, etc.)
    pub locale: String,

    /// Optional tags for filtering content
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    /// Installation mode (casc or containerless)
    #[serde(default)]
    pub mode: InstallationMode,
}

/// Update request body
///
/// Request to update an installed product to a newer version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRequest {
    /// Target build ID (optional, use latest if not specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_id: Option<u32>,

    /// Force update even if already up to date
    #[serde(default)]
    pub force: bool,
}

/// Repair request body
///
/// Request to repair a corrupted installation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairRequest {
    /// Force verification of all files (default: only missing/corrupted)
    #[serde(default)]
    pub force_verify_all: bool,
}

/// Verify request body
///
/// Request to verify installation integrity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyRequest {
    /// Verify all files (default: quick check)
    #[serde(default)]
    pub full_verify: bool,
}

/// Operation response with hypermedia links
///
/// Returned when an operation is created or queried.
/// Implements HATEOAS by including links to related resources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResponse {
    /// Unique operation identifier
    pub operation_id: Uuid,

    /// Target product code
    pub product_code: String,

    /// Type of operation
    pub operation_type: OperationType,

    /// Current operation state
    pub state: OperationState,

    /// Current progress (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<ProgressInfo>,

    /// Error details (if failed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,

    /// When operation was created
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// Last update timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,

    /// When operation started
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,

    /// When operation completed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,

    /// Hypermedia links
    #[serde(rename = "_links")]
    pub links: HashMap<String, Link>,
}

/// Progress information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressInfo {
    /// Current phase (e.g., "downloading", "verifying")
    pub phase: String,

    /// Percentage complete (0-100)
    pub percentage: f64,

    /// Bytes downloaded
    pub bytes_downloaded: u64,

    /// Total bytes to download
    pub bytes_total: u64,

    /// Files completed
    pub files_completed: u32,

    /// Total files
    pub files_total: u32,

    /// Download speed in bytes per second
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_speed_bps: Option<f64>,

    /// Estimated time remaining in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_seconds: Option<f64>,
}

/// Error information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInfo {
    /// Error code
    pub code: String,

    /// Human-readable error message
    pub message: String,

    /// Additional error details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// Hypermedia link
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    /// Link href (path or URL)
    pub href: String,

    /// Optional link type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

/// Product response
///
/// Information about a product and its current state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductResponse {
    /// Product code
    pub product_code: String,

    /// Product name
    pub name: String,

    /// Current product status
    pub status: ProductStatus,

    /// Installation path (if installed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_path: Option<String>,

    /// Installed version (if installed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Whether an update is available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_update_available: Option<bool>,

    /// Version number available for update
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_version: Option<String>,

    /// Hypermedia links
    #[serde(rename = "_links")]
    pub links: HashMap<String, Link>,
}

/// List of operations response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationListResponse {
    /// Operations matching query
    pub operations: Vec<OperationResponse>,

    /// Total count (for pagination)
    pub total: usize,

    /// Pagination links
    #[serde(rename = "_links")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<HashMap<String, Link>>,
}

/// List of products response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductListResponse {
    /// Products
    pub products: Vec<ProductResponse>,

    /// Hypermedia links
    #[serde(rename = "_links")]
    pub links: HashMap<String, Link>,
}

/// Health check response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Service status (healthy, degraded, unhealthy)
    pub status: String,

    /// Service version
    pub version: String,

    /// Uptime in seconds
    pub uptime_seconds: u64,

    /// Active operations count
    pub active_operations: usize,
}

/// Structured error response
///
/// Returned for all error conditions with proper HTTP status codes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// Error code
    pub error: String,

    /// Human-readable error message
    pub message: String,

    /// Additional error details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,

    /// HTTP status code
    pub status: u16,
}

/// Operation progress response for SSE streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationProgressResponse {
    /// Operation ID
    pub operation_id: Uuid,

    /// Current state
    pub state: OperationState,

    /// Progress information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<ProgressInfo>,

    /// Error information if failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
}

impl IntoResponse for OperationProgressResponse {
    fn into_response(self) -> Response {
        Json(self).into_response()
    }
}

impl From<Operation> for OperationResponse {
    fn from(op: Operation) -> Self {
        let mut links = HashMap::new();
        links.insert(
            "self".to_string(),
            Link {
                href: format!("/operations/{}", op.operation_id),
                type_: Some("application/json".to_string()),
            },
        );
        links.insert(
            "product".to_string(),
            Link {
                href: format!("/products/{}", op.product_code),
                type_: Some("application/json".to_string()),
            },
        );

        Self {
            operation_id: op.operation_id,
            product_code: op.product_code,
            operation_type: op.operation_type,
            state: op.state,
            progress: op.progress.map(|p| ProgressInfo {
                phase: p.phase,
                percentage: p.percentage,
                bytes_downloaded: p.bytes_downloaded,
                bytes_total: p.bytes_total,
                files_completed: p.files_completed as u32,
                files_total: p.files_total as u32,
                download_speed_bps: p.download_speed_bps.map(|s| s as f64),
                eta_seconds: p.eta_seconds.map(|e| e as f64),
            }),
            error: op.error.map(|e| ErrorInfo {
                code: e.code,
                message: e.message,
                details: e.details,
            }),
            created_at: op.created_at,
            updated_at: op.updated_at,
            started_at: op.started_at,
            completed_at: op.completed_at,
            links,
        }
    }
}

impl From<Product> for ProductResponse {
    fn from(product: Product) -> Self {
        let mut links = HashMap::new();
        links.insert(
            "self".to_string(),
            Link {
                href: format!("/products/{}", product.product_code),
                type_: Some("application/json".to_string()),
            },
        );

        Self {
            product_code: product.product_code.clone(),
            name: product.name,
            status: product.status,
            install_path: product.install_path,
            version: product.version,
            is_update_available: product.is_update_available,
            available_version: product.available_version,
            links,
        }
    }
}

impl Link {
    /// Create a simple link with just href
    #[must_use]
    pub fn new(href: String) -> Self {
        Self { href, type_: None }
    }

    /// Create a link with type
    #[must_use]
    pub fn with_type(href: String, type_: String) -> Self {
        Self {
            href,
            type_: Some(type_),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Priority;

    #[test]
    fn test_install_request_serialization() {
        let req = InstallRequest {
            build_id: Some(56313),
            install_path: "/games/wow".to_string(),
            region: "us".to_string(),
            locale: "enUS".to_string(),
            tags: vec!["Windows".to_string()],
            mode: InstallationMode::Casc,
        };

        let json = serde_json::to_string(&req).expect("Failed to serialize JSON");
        assert!(json.contains("56313"));
        assert!(json.contains("/games/wow"));
        assert!(json.contains("enUS"));
    }

    #[test]
    fn test_operation_response_conversion() {
        let op = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        let response = OperationResponse::from(op.clone());

        assert_eq!(response.operation_id, op.operation_id);
        assert_eq!(response.product_code, "wow");
        assert_eq!(response.operation_type, OperationType::Install);
        assert_eq!(response.state, OperationState::Queued);
        assert!(response.links.contains_key("self"));
        assert!(response.links.contains_key("product"));
    }

    #[test]
    fn test_error_response() {
        let err = ErrorResponse {
            error: "CONFLICT".to_string(),
            message: "Operation already in progress".to_string(),
            details: None,
            status: 409,
        };

        let json = serde_json::to_value(&err).expect("Failed to serialize to JSON value");
        assert_eq!(json["error"], "CONFLICT");
        assert_eq!(json["status"], 409);
    }

    #[test]
    fn test_health_response() {
        let health = HealthResponse {
            status: "healthy".to_string(),
            version: "0.1.0".to_string(),
            uptime_seconds: 3600,
            active_operations: 2,
        };

        let json = serde_json::to_string(&health).expect("Failed to serialize JSON");
        assert!(json.contains("healthy"));
        assert!(json.contains("0.1.0"));
    }
}
