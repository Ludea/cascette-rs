//! Error types for cascette-agent
//!
//! Provides comprehensive error handling for all agent operations using thiserror.
//! All errors are designed to provide clear context for debugging and user feedback.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

/// Agent service errors
#[derive(Debug, Error)]
pub enum AgentError {
    /// Configuration-related errors
    #[error("Configuration error: {0}")]
    Config(String),

    /// Database-related errors
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    // Future use: T081 (server startup)
    #[allow(dead_code)]
    /// HTTP server errors
    #[error("HTTP server error: {0}")]
    Http(String),

    // Future use: T081 (server startup with port binding)
    #[allow(dead_code)]
    /// Port binding errors
    #[error("Failed to bind to any port (tried: {ports:?}): {reason}")]
    PortBinding {
        /// Ports that were attempted
        ports: Vec<u16>,
        /// Reason for failure
        reason: String,
    },

    /// Installation library errors
    #[error("Installation error: {0}")]
    Installation(#[from] cascette_installation::InstallationError),

    /// Operation state errors
    #[error("Operation error: {0}")]
    Operation(#[from] OperationError),

    /// Product errors
    #[error("Product error: {0}")]
    Product(#[from] ProductError),

    /// IO errors
    #[error("IO error: {0}")]
    IoError(String),

    /// Serialization/deserialization errors
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Invalid operation parameters
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    /// Product not installed
    #[error("Product not installed: {0}")]
    ProductNotInstalled(String),

    /// Version downgrade rejected (FR-032)
    #[error("Cannot downgrade from {current_version} to {requested_version}")]
    DowngradeRejected {
        /// Current version
        current_version: String,
        /// Requested version
        requested_version: String,
    },

    /// Operation cancelled
    #[error("Operation cancelled")]
    OperationCancelled,

    /// Other errors
    #[error("{0}")]
    Other(String),
}

/// Operation-specific errors
#[derive(Debug, Error)]
pub enum OperationError {
    // Future use: Operation lookup handlers
    #[allow(dead_code)]
    /// Operation not found
    #[error("Operation not found: {0}")]
    NotFound(String),

    // Future use: FR-022 concurrent operation rejection (may already be used)
    #[allow(dead_code)]
    /// Operation already in progress (conflict - FR-022)
    #[error("Another operation is already in progress for product '{product}': {operation_id}")]
    Conflict {
        /// Product code
        product: String,
        /// Conflicting operation ID
        operation_id: String,
    },

    // Future use: State machine validation in handlers
    #[allow(dead_code)]
    /// Invalid state transition
    #[error("Invalid state transition: cannot move from {from} to {to}")]
    InvalidStateTransition {
        /// Current state
        from: String,
        /// Requested state
        to: String,
    },

    // Future use: Modification prevention for completed operations
    #[allow(dead_code)]
    /// Operation already completed
    #[error("Operation {0} has already completed and cannot be modified")]
    AlreadyCompleted(String),

    // Future use: Cancellation handling
    #[allow(dead_code)]
    /// Operation cancelled
    #[error("Operation {0} was cancelled")]
    Cancelled(String),

    // Future use: Request validation in handlers
    #[allow(dead_code)]
    /// Invalid operation request
    #[error("Invalid operation request: {0}")]
    InvalidRequest(String),
}

/// Product-specific errors
#[derive(Debug, Error)]
pub enum ProductError {
    /// Product not found
    #[error("Product not found: {0}")]
    NotFound(String),

    // Future use: Installation conflict detection
    #[allow(dead_code)]
    /// Product already installed
    #[error("Product '{0}' is already installed")]
    AlreadyInstalled(String),

    // Future use: Uninstall/repair/update handlers
    #[allow(dead_code)]
    /// Product not installed
    #[error("Product '{0}' is not installed")]
    NotInstalled(String),

    // Future use: Product code validation
    #[allow(dead_code)]
    /// Invalid product code
    #[error("Invalid product code: {0}")]
    InvalidCode(String),

    // Future use: T049 (version downgrade detection)
    #[allow(dead_code)]
    /// Version downgrade attempted (FR-032)
    #[error("Cannot downgrade product '{product}' from {current} to {target}")]
    DowngradeRejected {
        /// Product code
        product: String,
        /// Current version
        current: String,
        /// Target version
        target: String,
    },

    // Future use: Version validation
    #[allow(dead_code)]
    /// Invalid version
    #[error("Invalid version: {0}")]
    InvalidVersion(String),
}

/// Result type for agent operations
pub type Result<T> = std::result::Result<T, AgentError>;

/// JSON error response
#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<String>,
}

impl IntoResponse for AgentError {
    fn into_response(self) -> Response {
        let (status, error, details) = match &self {
            Self::Operation(OperationError::NotFound(_)) => {
                (StatusCode::NOT_FOUND, "Not Found", Some(self.to_string()))
            }
            Self::Operation(OperationError::Conflict { .. }) => {
                (StatusCode::CONFLICT, "Conflict", Some(self.to_string()))
            }
            Self::Operation(OperationError::AlreadyCompleted(_)) => (
                StatusCode::CONFLICT,
                "Already Completed",
                Some(self.to_string()),
            ),
            Self::Operation(OperationError::InvalidStateTransition { .. }) => (
                StatusCode::BAD_REQUEST,
                "Bad Request",
                Some(self.to_string()),
            ),
            Self::Operation(OperationError::InvalidRequest(_)) => (
                StatusCode::BAD_REQUEST,
                "Bad Request",
                Some(self.to_string()),
            ),
            Self::Product(ProductError::NotFound(_)) => {
                (StatusCode::NOT_FOUND, "Not Found", Some(self.to_string()))
            }
            Self::Product(ProductError::AlreadyInstalled(_)) => {
                (StatusCode::CONFLICT, "Conflict", Some(self.to_string()))
            }
            Self::Product(ProductError::NotInstalled(_)) => (
                StatusCode::BAD_REQUEST,
                "Bad Request",
                Some(self.to_string()),
            ),
            Self::Product(ProductError::DowngradeRejected { .. }) => (
                StatusCode::BAD_REQUEST,
                "Downgrade Not Allowed",
                Some(self.to_string()),
            ),
            Self::Product(ProductError::InvalidCode(_)) => (
                StatusCode::BAD_REQUEST,
                "Bad Request",
                Some(self.to_string()),
            ),
            Self::DowngradeRejected { .. } => (
                StatusCode::BAD_REQUEST,
                "Downgrade Not Allowed",
                Some(self.to_string()),
            ),
            Self::InvalidOperation(_) => (
                StatusCode::BAD_REQUEST,
                "Bad Request",
                Some(self.to_string()),
            ),
            Self::ProductNotInstalled(_) => (
                StatusCode::BAD_REQUEST,
                "Product Not Installed",
                Some(self.to_string()),
            ),
            Self::OperationCancelled => (StatusCode::OK, "Cancelled", Some(self.to_string())),
            Self::Config(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                Some(self.to_string()),
            ),
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                None,
            ),
            Self::Serialization(_) => (
                StatusCode::BAD_REQUEST,
                "Bad Request",
                Some(self.to_string()),
            ),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                None,
            ),
        };

        let body = Json(ErrorResponse {
            error: error.to_string(),
            details,
        });

        (status, body).into_response()
    }
}

impl From<anyhow::Error> for AgentError {
    fn from(err: anyhow::Error) -> Self {
        Self::Other(err.to_string())
    }
}

impl From<std::io::Error> for AgentError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err.to_string())
    }
}

impl From<serde_json::Error> for AgentError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialization(err.to_string())
    }
}

impl From<toml::de::Error> for AgentError {
    fn from(err: toml::de::Error) -> Self {
        Self::Config(err.to_string())
    }
}

impl From<toml::ser::Error> for AgentError {
    fn from(err: toml::ser::Error) -> Self {
        Self::Serialization(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_error() {
        let err = AgentError::Config("Invalid bind address".to_string());
        assert_eq!(err.to_string(), "Configuration error: Invalid bind address");
    }

    #[test]
    fn test_port_binding_error() {
        let err = AgentError::PortBinding {
            ports: vec![1120, 6881, 6882, 6883],
            reason: "Address already in use".to_string(),
        };
        assert!(err.to_string().contains("Failed to bind to any port"));
        assert!(err.to_string().contains("1120"));
    }

    #[test]
    fn test_operation_conflict() {
        let err = OperationError::Conflict {
            product: "wow".to_string(),
            operation_id: "abc123".to_string(),
        };
        assert!(err.to_string().contains("already in progress"));
        assert!(err.to_string().contains("wow"));
        assert!(err.to_string().contains("abc123"));
    }

    #[test]
    fn test_invalid_state_transition() {
        let err = OperationError::InvalidStateTransition {
            from: "completed".to_string(),
            to: "in_progress".to_string(),
        };
        assert!(
            err.to_string()
                .contains("cannot move from completed to in_progress")
        );
    }

    #[test]
    fn test_product_not_found() {
        let err = ProductError::NotFound("wow_classic".to_string());
        assert_eq!(err.to_string(), "Product not found: wow_classic");
    }

    #[test]
    fn test_product_downgrade_rejected() {
        let err = ProductError::DowngradeRejected {
            product: "wow".to_string(),
            current: "10.2.0.52607".to_string(),
            target: "10.1.0.52000".to_string(),
        };
        assert!(err.to_string().contains("Cannot downgrade"));
        assert!(err.to_string().contains("10.2.0"));
        assert!(err.to_string().contains("10.1.0"));
    }

    #[test]
    fn test_operation_already_completed() {
        let err = OperationError::AlreadyCompleted("op123".to_string());
        assert!(err.to_string().contains("already completed"));
        assert!(err.to_string().contains("op123"));
    }

    #[test]
    fn test_product_already_installed() {
        let err = ProductError::AlreadyInstalled("wow".to_string());
        assert!(err.to_string().contains("already installed"));
    }

    #[test]
    fn test_product_not_installed() {
        let err = ProductError::NotInstalled("wow_classic".to_string());
        assert!(err.to_string().contains("not installed"));
    }

    #[test]
    fn test_error_conversion_from_anyhow() {
        let anyhow_err = anyhow::anyhow!("Something went wrong");
        let agent_err: AgentError = anyhow_err.into();
        assert!(agent_err.to_string().contains("Something went wrong"));
    }

    #[test]
    fn test_error_conversion_from_serde_json() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid json")
            .expect_err("Should be parse error");
        let agent_err: AgentError = json_err.into();
        assert!(matches!(agent_err, AgentError::Serialization(_)));
    }

    #[test]
    fn test_into_response_not_found() {
        let err = AgentError::Operation(OperationError::NotFound("test-id".to_string()));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_into_response_conflict() {
        let err = AgentError::Operation(OperationError::Conflict {
            product: "wow".to_string(),
            operation_id: "test-id".to_string(),
        });
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[test]
    fn test_into_response_bad_request() {
        let err = AgentError::Operation(OperationError::InvalidRequest("bad data".to_string()));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
