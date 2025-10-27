//! Operation model for agent service
//!
//! Represents a long-running task (install, update, repair, verify, uninstall).
//! Based on data-model.md Operation entity specification.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::progress::Progress;

/// Operation type enum
///
/// Defines the type of operation being performed on a product.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationType {
    /// Install new product
    Install,
    /// Update installed product
    Update,
    /// Repair corrupted installation
    Repair,
    /// Verify installation integrity
    Verify,
    /// Remove installed product
    Uninstall,
}

impl std::fmt::Display for OperationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Install => write!(f, "install"),
            Self::Update => write!(f, "update"),
            Self::Repair => write!(f, "repair"),
            Self::Verify => write!(f, "verify"),
            Self::Uninstall => write!(f, "uninstall"),
        }
    }
}

/// Operation state enum
///
/// Represents the current execution state of an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    /// Waiting to begin
    Queued,
    /// Preparing resources
    Initializing,
    /// Downloading content
    Downloading,
    /// Checking integrity
    Verifying,
    /// Finished successfully
    Complete,
    /// Encountered fatal error
    Failed,
    /// Cancelled by user
    Cancelled,
}

/// Operation priority enum
///
/// Determines execution priority and resource allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// Background operation
    Low,
    /// Standard priority
    Normal,
    /// User-initiated, needs fast execution
    High,
}

/// Error information for failed operations
///
/// Captures detailed error context for debugging and user feedback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInfo {
    /// Error code (e.g., `DOWNLOAD_FAILED`, `VERIFICATION_FAILED`)
    pub code: String,

    /// Human-readable error message
    pub message: String,

    /// Additional error details (structured data)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,

    /// When the error occurred
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Operation model
///
/// Represents a long-running task with state tracking and progress monitoring.
/// Implements validation rules and state transitions from data-model.md.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    /// Unique operation identifier
    pub operation_id: Uuid,

    /// Target product code
    pub product_code: String,

    /// Type of operation
    pub operation_type: OperationType,

    /// Current operation state
    pub state: OperationState,

    /// Operation priority
    pub priority: Priority,

    /// Operation-specific parameters (`install_path`, `build_id`, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,

    /// Operation metadata (verification results, uninstall stats, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,

    /// Current progress metrics
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<Progress>,

    /// Error details if failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,

    /// When operation was created
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// Last state or progress update
    pub updated_at: chrono::DateTime<chrono::Utc>,

    /// When operation started executing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,

    /// When operation finished
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

// Future use: T078 (main.rs operation management)
#[allow(dead_code)]
impl Operation {
    /// Create a new operation in Queued state
    #[must_use]
    pub fn new(product_code: String, operation_type: OperationType, priority: Priority) -> Self {
        let now = chrono::Utc::now();
        Self {
            operation_id: Uuid::new_v4(),
            product_code,
            operation_type,
            state: OperationState::Queued,
            priority,
            parameters: None,
            metadata: None,
            progress: None,
            error: None,
            created_at: now,
            updated_at: now,
            started_at: None,
            completed_at: None,
        }
    }

    /// Validate operation data according to data-model.md rules
    ///
    /// # Validation Rules
    ///
    /// - If state is Complete, then `completed_at` must not be null
    /// - If state is Failed, then error must not be null
    /// - If state is Queued, then `started_at` and `completed_at` must be null
    /// - If state is Initializing/Downloading/Verifying, then `started_at` must not be null
    pub fn validate(&self) -> Result<(), String> {
        match self.state {
            OperationState::Complete => {
                if self.completed_at.is_none() {
                    return Err("completed_at must be set for Complete operations".to_string());
                }
            }
            OperationState::Failed => {
                if self.error.is_none() {
                    return Err("error must be set for Failed operations".to_string());
                }
                if self.completed_at.is_none() {
                    return Err("completed_at must be set for Failed operations".to_string());
                }
            }
            OperationState::Queued => {
                if self.started_at.is_some() {
                    return Err("started_at must be null for Queued operations".to_string());
                }
                if self.completed_at.is_some() {
                    return Err("completed_at must be null for Queued operations".to_string());
                }
            }
            OperationState::Initializing
            | OperationState::Downloading
            | OperationState::Verifying => {
                if self.started_at.is_none() {
                    return Err(format!(
                        "started_at must be set for {:?} operations",
                        self.state
                    ));
                }
            }
            OperationState::Cancelled => {
                if self.completed_at.is_none() {
                    return Err("completed_at must be set for Cancelled operations".to_string());
                }
            }
        }

        Ok(())
    }

    /// Check if a state transition is valid
    ///
    /// Implements state transition rules from data-model.md:
    /// - Queued → Initializing, Cancelled
    /// - Initializing → Downloading, Failed
    /// - Downloading → Verifying, Failed
    /// - Verifying → Complete, Failed, Downloading (retry)
    /// - Complete/Failed/Cancelled are terminal states
    #[must_use]
    pub fn can_transition_to(&self, new_state: OperationState) -> bool {
        match (self.state, new_state) {
            // Queued can transition to Initializing or Cancelled
            (OperationState::Queued, OperationState::Initializing) => true,
            (OperationState::Queued, OperationState::Cancelled) => true,

            // Initializing can transition to Downloading or Failed
            (OperationState::Initializing, OperationState::Downloading) => true,
            (OperationState::Initializing, OperationState::Failed) => true,
            (OperationState::Initializing, OperationState::Cancelled) => true,

            // Downloading can transition to Verifying or Failed
            (OperationState::Downloading, OperationState::Verifying) => true,
            (OperationState::Downloading, OperationState::Failed) => true,
            (OperationState::Downloading, OperationState::Cancelled) => true,

            // Verifying can transition to Complete, Failed, or back to Downloading (retry)
            (OperationState::Verifying, OperationState::Complete) => true,
            (OperationState::Verifying, OperationState::Failed) => true,
            (OperationState::Verifying, OperationState::Downloading) => true,
            (OperationState::Verifying, OperationState::Cancelled) => true,

            // Terminal states cannot transition
            (OperationState::Complete, _) => false,
            (OperationState::Failed, _) => false,
            (OperationState::Cancelled, _) => false,

            // Same state is always valid (no-op)
            (a, b) if a == b => true,

            // All other transitions are invalid
            _ => false,
        }
    }

    /// Check if operation is in a terminal state
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            OperationState::Complete | OperationState::Failed | OperationState::Cancelled
        )
    }

    /// Check if operation is in an active (non-terminal) state
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.is_terminal()
    }

    /// Update the operation state with timestamp
    pub fn set_state(&mut self, new_state: OperationState) {
        self.state = new_state;
        self.updated_at = chrono::Utc::now();

        // Set started_at when transitioning to Initializing
        if new_state == OperationState::Initializing && self.started_at.is_none() {
            self.started_at = Some(self.updated_at);
        }

        // Set completed_at when transitioning to terminal state
        if self.is_terminal() && self.completed_at.is_none() {
            self.completed_at = Some(self.updated_at);
        }
    }

    /// Set operation error and transition to Failed state
    pub fn set_error(&mut self, code: String, message: String, details: Option<serde_json::Value>) {
        self.error = Some(ErrorInfo {
            code,
            message,
            details,
            timestamp: chrono::Utc::now(),
        });
        self.set_state(OperationState::Failed);
    }

    /// Update operation progress
    pub fn set_progress(&mut self, progress: Progress) {
        self.progress = Some(progress);
        self.updated_at = chrono::Utc::now();
    }
}

impl Default for Priority {
    fn default() -> Self {
        Self::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_operation() {
        let operation = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        assert_eq!(operation.product_code, "wow");
        assert_eq!(operation.operation_type, OperationType::Install);
        assert_eq!(operation.state, OperationState::Queued);
        assert_eq!(operation.priority, Priority::Normal);
        assert!(operation.parameters.is_none());
        assert!(operation.metadata.is_none());
        assert!(operation.validate().is_ok());
    }

    #[test]
    fn test_queued_validation() {
        let operation = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        assert!(operation.validate().is_ok());

        let mut invalid = operation.clone();
        invalid.started_at = Some(chrono::Utc::now());
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_complete_validation() {
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        operation.state = OperationState::Complete;

        // Missing completed_at
        assert!(operation.validate().is_err());

        operation.completed_at = Some(chrono::Utc::now());
        assert!(operation.validate().is_ok());
    }

    #[test]
    fn test_failed_validation() {
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        operation.state = OperationState::Failed;

        // Missing error
        assert!(operation.validate().is_err());

        operation.error = Some(ErrorInfo {
            code: "TEST_ERROR".to_string(),
            message: "Test error".to_string(),
            details: None,
            timestamp: chrono::Utc::now(),
        });
        operation.completed_at = Some(chrono::Utc::now());
        assert!(operation.validate().is_ok());
    }

    #[test]
    fn test_state_transitions() {
        let operation = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        // Queued → Initializing (valid)
        assert!(operation.can_transition_to(OperationState::Initializing));

        // Queued → Downloading (invalid, must go through Initializing)
        assert!(!operation.can_transition_to(OperationState::Downloading));

        let mut operation = operation;
        operation.set_state(OperationState::Initializing);

        // Initializing → Downloading (valid)
        assert!(operation.can_transition_to(OperationState::Downloading));

        operation.set_state(OperationState::Downloading);

        // Downloading → Verifying (valid)
        assert!(operation.can_transition_to(OperationState::Verifying));

        operation.set_state(OperationState::Verifying);

        // Verifying → Complete (valid)
        assert!(operation.can_transition_to(OperationState::Complete));

        // Verifying → Downloading (valid, retry)
        assert!(operation.can_transition_to(OperationState::Downloading));

        operation.set_state(OperationState::Complete);

        // Terminal states cannot transition
        assert!(!operation.can_transition_to(OperationState::Downloading));
    }

    #[test]
    fn test_terminal_states() {
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        assert!(!operation.is_terminal());
        assert!(operation.is_active());

        operation.set_state(OperationState::Complete);
        assert!(operation.is_terminal());
        assert!(!operation.is_active());
    }

    #[test]
    fn test_set_error() {
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        operation.set_error(
            "DOWNLOAD_FAILED".to_string(),
            "Failed to download archive".to_string(),
            None,
        );

        assert_eq!(operation.state, OperationState::Failed);
        assert!(operation.error.is_some());
        assert_eq!(
            operation.error.as_ref().expect("Error should exist").code,
            "DOWNLOAD_FAILED"
        );
        assert!(operation.completed_at.is_some());
    }

    #[test]
    fn test_auto_timestamps() {
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        assert!(operation.started_at.is_none());
        assert!(operation.completed_at.is_none());

        operation.set_state(OperationState::Initializing);
        assert!(operation.started_at.is_some());
        assert!(operation.completed_at.is_none());

        operation.set_state(OperationState::Complete);
        assert!(operation.completed_at.is_some());
    }
}
