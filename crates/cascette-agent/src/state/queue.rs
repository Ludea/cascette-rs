//! Operation queue for state management
//!
//! Provides CRUD operations for operations with state management and validation.
//! Enforces business rules like single active operation per product (FR-022)
//! and automatic cleanup of old operations (FR-031).

use rusqlite::{Row, params};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::error::{AgentError, OperationError, Result};
use crate::models::{ErrorInfo, Operation, OperationState, OperationType, Priority, Progress};
use crate::state::db::Database;

/// Operation queue managing operation state and persistence
pub struct OperationQueue {
    db: Arc<Mutex<Database>>,
}

// Future use: FR-030 (persistent operation state), T078 (main.rs)
#[allow(dead_code)]
impl OperationQueue {
    /// Create a new operation queue
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self { db }
    }

    /// Create a new operation
    ///
    /// Validates that no other active operation exists for the same product (FR-022).
    /// Returns error if validation fails or if a conflicting operation exists.
    pub fn create(&self, operation: &Operation) -> Result<()> {
        operation
            .validate()
            .map_err(|e| AgentError::Operation(OperationError::InvalidRequest(e)))?;

        // Check for existing active operation on same product (FR-022)
        if let Some(existing_id) = self.get_active_operation_for_product(&operation.product_code)? {
            return Err(AgentError::Operation(OperationError::Conflict {
                product: operation.product_code.clone(),
                operation_id: existing_id.to_string(),
            }));
        }

        let db = self.db.lock().expect("Database lock poisoned");
        let conn = db.connection();

        conn.execute(
            "INSERT INTO operations (
                operation_id, product_code, operation_type, state, priority,
                parameters_json, progress_json, error_json, created_at, updated_at,
                started_at, completed_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                operation.operation_id.to_string(),
                operation.product_code,
                operation_type_to_string(operation.operation_type),
                state_to_string(operation.state),
                priority_to_string(operation.priority),
                operation
                    .parameters
                    .as_ref()
                    .and_then(|p| serde_json::to_string(p).ok()),
                operation
                    .progress
                    .as_ref()
                    .and_then(|p| serde_json::to_string(p).ok()),
                operation
                    .error
                    .as_ref()
                    .and_then(|e| serde_json::to_string(e).ok()),
                operation.created_at.to_rfc3339(),
                operation.updated_at.to_rfc3339(),
                operation.started_at.map(|t| t.to_rfc3339()),
                operation.completed_at.map(|t| t.to_rfc3339()),
            ],
        )?;

        Ok(())
    }

    /// Get an operation by ID
    pub fn get(&self, operation_id: Uuid) -> Result<Operation> {
        let db = self.db.lock().expect("Database lock poisoned");
        let conn = db.connection();

        let operation = conn
            .query_row(
                "SELECT operation_id, product_code, operation_type, state, priority,
                    parameters_json, progress_json, error_json, created_at, updated_at,
                    started_at, completed_at
             FROM operations
             WHERE operation_id = ?1",
                params![operation_id.to_string()],
                parse_operation_row,
            )
            .map_err(|e| {
                if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
                    AgentError::Operation(OperationError::NotFound(operation_id.to_string()))
                } else {
                    AgentError::Database(e)
                }
            })?;

        Ok(operation)
    }

    /// Update an existing operation
    ///
    /// Validates state transition before updating.
    pub fn update(&self, operation: &Operation) -> Result<()> {
        operation
            .validate()
            .map_err(|e| AgentError::Operation(OperationError::InvalidRequest(e)))?;

        // Get current operation to validate state transition
        let current = self.get(operation.operation_id)?;

        // Terminal states cannot transition
        if current.is_terminal() {
            return Err(AgentError::Operation(OperationError::AlreadyCompleted(
                operation.operation_id.to_string(),
            )));
        }

        if !current.can_transition_to(operation.state) {
            return Err(AgentError::Operation(
                OperationError::InvalidStateTransition {
                    from: format!("{:?}", current.state),
                    to: format!("{:?}", operation.state),
                },
            ));
        }

        let db = self.db.lock().expect("Database lock poisoned");
        let conn = db.connection();

        let rows_affected = conn.execute(
            "UPDATE operations
             SET state = ?2, priority = ?3, parameters_json = ?4, progress_json = ?5, error_json = ?6,
                 updated_at = ?7, started_at = ?8, completed_at = ?9
             WHERE operation_id = ?1",
            params![
                operation.operation_id.to_string(),
                state_to_string(operation.state),
                priority_to_string(operation.priority),
                operation
                    .parameters
                    .as_ref()
                    .and_then(|p| serde_json::to_string(p).ok()),
                operation
                    .progress
                    .as_ref()
                    .and_then(|p| serde_json::to_string(p).ok()),
                operation
                    .error
                    .as_ref()
                    .and_then(|e| serde_json::to_string(e).ok()),
                operation.updated_at.to_rfc3339(),
                operation.started_at.map(|t| t.to_rfc3339()),
                operation.completed_at.map(|t| t.to_rfc3339()),
            ],
        )?;

        if rows_affected == 0 {
            return Err(AgentError::Operation(OperationError::NotFound(
                operation.operation_id.to_string(),
            )));
        }

        Ok(())
    }

    /// Delete an operation
    pub fn delete(&self, operation_id: Uuid) -> Result<()> {
        let db = self.db.lock().expect("Database lock poisoned");
        let conn = db.connection();

        let rows_affected = conn.execute(
            "DELETE FROM operations WHERE operation_id = ?1",
            params![operation_id.to_string()],
        )?;

        if rows_affected == 0 {
            return Err(AgentError::Operation(OperationError::NotFound(
                operation_id.to_string(),
            )));
        }

        Ok(())
    }

    /// List all operations
    pub fn list(&self) -> Result<Vec<Operation>> {
        let db = self.db.lock().expect("Database lock poisoned");
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT operation_id, product_code, operation_type, state, priority,
                    parameters_json, progress_json, error_json, created_at, updated_at,
                    started_at, completed_at
             FROM operations
             ORDER BY created_at DESC",
        )?;

        let operations = stmt
            .query_map([], parse_operation_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(operations)
    }

    /// List operations by state
    pub fn list_by_state(&self, state: OperationState) -> Result<Vec<Operation>> {
        let db = self.db.lock().expect("Database lock poisoned");
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT operation_id, product_code, operation_type, state, priority,
                    parameters_json, progress_json, error_json, created_at, updated_at,
                    started_at, completed_at
             FROM operations
             WHERE state = ?1
             ORDER BY created_at DESC",
        )?;

        let operations = stmt
            .query_map(params![state_to_string(state)], |row| {
                parse_operation_row(row)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(operations)
    }

    /// List operations by product
    pub fn list_by_product(&self, product_code: &str) -> Result<Vec<Operation>> {
        let db = self.db.lock().expect("Database lock poisoned");
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT operation_id, product_code, operation_type, state, priority,
                    parameters_json, progress_json, error_json, created_at, updated_at,
                    started_at, completed_at
             FROM operations
             WHERE product_code = ?1
             ORDER BY created_at DESC",
        )?;

        let operations = stmt
            .query_map(params![product_code], parse_operation_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(operations)
    }

    /// Get active operation for a product, if any (FR-022 enforcement)
    ///
    /// Returns the operation ID if there is a non-terminal operation for the product.
    pub fn get_active_operation_for_product(&self, product_code: &str) -> Result<Option<Uuid>> {
        let db = self.db.lock().expect("Database lock poisoned");
        let conn = db.connection();

        let result = conn.query_row(
            "SELECT operation_id FROM operations
             WHERE product_code = ?1
             AND state IN ('Queued', 'Initializing', 'Downloading', 'Verifying')
             ORDER BY created_at DESC
             LIMIT 1",
            params![product_code],
            |row| {
                let id_str: String = row.get(0)?;
                Ok(id_str)
            },
        );

        match result {
            Ok(id_str) => {
                let uuid = Uuid::parse_str(&id_str)
                    .map_err(|e| AgentError::Other(format!("Invalid UUID in database: {e}")))?;
                Ok(Some(uuid))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AgentError::Database(e)),
        }
    }

    /// Find interrupted operations for resume (T076)
    ///
    /// Returns all operations in non-terminal states that were in progress
    /// when the service stopped. These operations should be resumed on startup.
    pub fn find_interrupted_operations(&self) -> Result<Vec<Operation>> {
        let db = self.db.lock().expect("Database lock poisoned");
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT operation_id, product_code, operation_type, state, priority,
                parameters_json, progress_json, error_json, created_at, updated_at,
                started_at, completed_at
             FROM operations
             WHERE state IN ('Queued', 'Initializing', 'Downloading', 'Verifying')
             ORDER BY created_at ASC",
        )?;

        let operations = stmt
            .query_map([], parse_operation_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(operations)
    }

    /// Clean up old operations (FR-031: 90-day retention)
    ///
    /// Removes terminal operations older than 90 days.
    /// Returns the number of operations removed.
    pub fn cleanup_old_operations(&self) -> Result<usize> {
        let mut db = self.db.lock().expect("Database lock poisoned");
        let count = db.cleanup_old_operations()?;
        Ok(count)
    }

    // Wrapper methods for API compatibility
    // Future use: FR-030 (persistent operation state)

    #[allow(dead_code)]
    /// Alias for `create()` - for API compatibility
    pub fn create_operation(&self, operation: &Operation) -> Result<()> {
        self.create(operation)
    }

    #[allow(dead_code)]
    /// Alias for `get()` - for API compatibility
    pub fn get_operation(&self, operation_id: Uuid) -> Result<Operation> {
        self.get(operation_id)
    }

    #[allow(dead_code)]
    /// Alias for `update()` - for API compatibility
    pub fn update_operation(&self, operation: &Operation) -> Result<()> {
        self.update(operation)
    }

    #[allow(dead_code)]
    /// Alias for `list()` - for API compatibility
    pub fn list_operations(&self) -> Result<Vec<Operation>> {
        self.list()
    }

    #[allow(dead_code)]
    /// Count total number of operations
    pub fn count_operations(&self) -> Result<usize> {
        let db = self.db.lock().expect("Database lock poisoned");
        let conn = db.connection();

        let count: i64 = conn.query_row("SELECT COUNT(*) FROM operations", [], |row| row.get(0))?;

        Ok(count as usize)
    }

    #[allow(dead_code)]
    /// Count active (non-terminal) operations
    pub fn active_operations_count(&self) -> Result<usize> {
        let db = self.db.lock().expect("Database lock poisoned");
        let conn = db.connection();

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM operations
             WHERE state IN ('Queued', 'Initializing', 'Downloading', 'Verifying')",
            [],
            |row| row.get(0),
        )?;

        Ok(count as usize)
    }
}

/// Implementation of runner::OperationQueue trait for background execution
#[async_trait::async_trait]
impl crate::executor::runner::OperationQueue for OperationQueue {
    async fn dequeue(&self) -> Option<Operation> {
        // Get the next queued operation ordered by priority and creation time
        let db = self.db.lock().expect("Database lock poisoned");
        let conn = db.connection();

        let mut stmt = conn
            .prepare(
                "SELECT operation_id, product_code, operation_type, state, priority,
                    parameters_json, progress_json, error_json, created_at, updated_at,
                    started_at, completed_at
                 FROM operations
                 WHERE state = 'Queued'
                 ORDER BY
                    CASE priority
                        WHEN 'High' THEN 1
                        WHEN 'Normal' THEN 2
                        WHEN 'Low' THEN 3
                    END,
                    created_at ASC
                 LIMIT 1",
            )
            .expect("Failed to prepare statement");

        let operation = stmt.query_row([], parse_operation_row).ok()?;

        // Transition to Initializing state
        let mut op = operation;
        op.set_state(OperationState::Initializing);

        // Update in database
        drop(stmt);
        drop(db);
        let _ = self.update(&op);

        Some(op)
    }

    async fn complete(&self, operation_id: uuid::Uuid, final_state: OperationState) {
        if let Ok(mut operation) = self.get(operation_id) {
            operation.set_state(final_state);
            let _ = self.update(&operation);
        }
    }

    async fn resume_interrupted(&self) -> Vec<Operation> {
        self.find_interrupted_operations().unwrap_or_default()
    }
}

/// Implementation of ProgressReporter trait for background execution
impl crate::executor::ProgressReporter for OperationQueue {
    fn report_progress(&self, operation_id: uuid::Uuid, progress: Progress) {
        if let Ok(mut operation) = self.get(operation_id) {
            operation.set_progress(progress);
            let _ = self.update(&operation);
        }
    }

    fn report_state_change(&self, operation_id: uuid::Uuid, new_state: OperationState) {
        if let Ok(mut operation) = self.get(operation_id) {
            operation.set_state(new_state);
            let _ = self.update(&operation);
        }
    }

    fn report_error(
        &self,
        operation_id: uuid::Uuid,
        error_code: String,
        error_message: String,
        details: Option<serde_json::Value>,
    ) {
        if let Ok(mut operation) = self.get(operation_id) {
            operation.set_error(error_code, error_message, details);
            operation.set_state(OperationState::Failed);
            let _ = self.update(&operation);
        }
    }
}

/// Parse an operation from a database row
fn parse_operation_row(row: &Row) -> rusqlite::Result<Operation> {
    let operation_id_str: String = row.get(0)?;
    let operation_id = Uuid::parse_str(&operation_id_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?;

    let parameters_json: Option<String> = row.get(5)?;
    let parameters = parameters_json
        .map(|json| serde_json::from_str::<serde_json::Value>(&json))
        .transpose()
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
        })?;

    let progress_json: Option<String> = row.get(6)?;
    let progress = progress_json
        .map(|json| serde_json::from_str::<Progress>(&json))
        .transpose()
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e))
        })?;

    let error_json: Option<String> = row.get(7)?;
    let error = error_json
        .map(|json| serde_json::from_str::<ErrorInfo>(&json))
        .transpose()
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(e))
        })?;

    Ok(Operation {
        operation_id,
        product_code: row.get(1)?,
        operation_type: string_to_operation_type(&row.get::<_, String>(2)?).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
            )
        })?,
        state: string_to_state(&row.get::<_, String>(3)?).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
            )
        })?,
        priority: string_to_priority(&row.get::<_, String>(4)?).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
            )
        })?,
        parameters,
        progress,
        error,
        created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    8,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?
            .with_timezone(&chrono::Utc),
        updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(9)?)
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    9,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?
            .with_timezone(&chrono::Utc),
        started_at: row
            .get::<_, Option<String>>(10)?
            .map(|s| chrono::DateTime::parse_from_rfc3339(&s))
            .transpose()
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    10,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?
            .map(|dt| dt.with_timezone(&chrono::Utc)),
        completed_at: row
            .get::<_, Option<String>>(11)?
            .map(|s| chrono::DateTime::parse_from_rfc3339(&s))
            .transpose()
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    11,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?
            .map(|dt| dt.with_timezone(&chrono::Utc)),
        metadata: None,
    })
}

/// Convert `OperationType` to database string
fn operation_type_to_string(op_type: OperationType) -> &'static str {
    match op_type {
        OperationType::Install => "Install",
        OperationType::Update => "Update",
        OperationType::Repair => "Repair",
        OperationType::Verify => "Verify",
        OperationType::Uninstall => "Uninstall",
    }
}

/// Convert database string to `OperationType`
fn string_to_operation_type(s: &str) -> std::result::Result<OperationType, String> {
    match s {
        "Install" => Ok(OperationType::Install),
        "Update" => Ok(OperationType::Update),
        "Repair" => Ok(OperationType::Repair),
        "Verify" => Ok(OperationType::Verify),
        "Uninstall" => Ok(OperationType::Uninstall),
        _ => Err(format!("Invalid operation type: {s}")),
    }
}

/// Convert `OperationState` to database string
fn state_to_string(state: OperationState) -> &'static str {
    match state {
        OperationState::Queued => "Queued",
        OperationState::Initializing => "Initializing",
        OperationState::Downloading => "Downloading",
        OperationState::Verifying => "Verifying",
        OperationState::Complete => "Complete",
        OperationState::Failed => "Failed",
        OperationState::Cancelled => "Cancelled",
    }
}

/// Convert database string to `OperationState`
fn string_to_state(s: &str) -> std::result::Result<OperationState, String> {
    match s {
        "Queued" => Ok(OperationState::Queued),
        "Initializing" => Ok(OperationState::Initializing),
        "Downloading" => Ok(OperationState::Downloading),
        "Verifying" => Ok(OperationState::Verifying),
        "Complete" => Ok(OperationState::Complete),
        "Failed" => Ok(OperationState::Failed),
        "Cancelled" => Ok(OperationState::Cancelled),
        _ => Err(format!("Invalid operation state: {s}")),
    }
}

/// Convert Priority to database string
fn priority_to_string(priority: Priority) -> &'static str {
    match priority {
        Priority::Low => "Low",
        Priority::Normal => "Normal",
        Priority::High => "High",
    }
}

/// Convert database string to Priority
fn string_to_priority(s: &str) -> std::result::Result<Priority, String> {
    match s {
        "Low" => Ok(Priority::Low),
        "Normal" => Ok(Priority::Normal),
        "High" => Ok(Priority::High),
        _ => Err(format!("Invalid priority: {s}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_queue() -> OperationQueue {
        let db = Database::in_memory().expect("Failed to create test database");

        // Insert a test product
        db.connection()
            .execute(
                "INSERT INTO products (product_code, name, status, created_at, updated_at)
             VALUES ('wow', 'World of Warcraft', 'Available', datetime('now'), datetime('now'))",
                [],
            )
            .expect("Failed to insert test product");

        OperationQueue::new(Arc::new(Mutex::new(db)))
    }

    #[test]
    fn test_create_operation() {
        let queue = create_test_queue();
        let operation = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        let result = queue.create(&operation);
        assert!(result.is_ok());
    }

    #[test]
    fn test_conflict_detection() {
        let queue = create_test_queue();
        let op1 = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        queue.create(&op1).expect("Failed to create operation");

        // Try to create another operation for the same product
        let op2 = Operation::new("wow".to_string(), OperationType::Update, Priority::Normal);
        let result = queue.create(&op2);

        assert!(result.is_err());
        assert!(matches!(
            result.expect_err("Should be error"),
            AgentError::Operation(OperationError::Conflict { .. })
        ));
    }

    #[test]
    fn test_get_operation() {
        let queue = create_test_queue();
        let operation = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        queue
            .create(&operation)
            .expect("Failed to create operation");

        let retrieved = queue
            .get(operation.operation_id)
            .expect("Failed to get operation");
        assert_eq!(retrieved.operation_id, operation.operation_id);
        assert_eq!(retrieved.product_code, "wow");
        assert_eq!(retrieved.operation_type, OperationType::Install);
    }

    #[test]
    fn test_get_nonexistent_operation() {
        let queue = create_test_queue();

        let result = queue.get(Uuid::new_v4());
        assert!(result.is_err());
        assert!(matches!(
            result.expect_err("Should be error"),
            AgentError::Operation(OperationError::NotFound(_))
        ));
    }

    #[test]
    fn test_update_operation() {
        let queue = create_test_queue();
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        queue
            .create(&operation)
            .expect("Failed to create operation");

        // Update to Initializing
        operation.set_state(OperationState::Initializing);
        let result = queue.update(&operation);
        assert!(result.is_ok());

        let retrieved = queue
            .get(operation.operation_id)
            .expect("Failed to get operation");
        assert_eq!(retrieved.state, OperationState::Initializing);
        assert!(retrieved.started_at.is_some());
    }

    #[test]
    fn test_update_terminal_operation() {
        let queue = create_test_queue();
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        queue
            .create(&operation)
            .expect("Failed to create operation");

        // Move to terminal state
        operation.set_state(OperationState::Initializing);
        queue
            .update(&operation)
            .expect("Failed to update operation");
        operation.set_state(OperationState::Downloading);
        queue
            .update(&operation)
            .expect("Failed to update operation");
        operation.set_state(OperationState::Verifying);
        queue
            .update(&operation)
            .expect("Failed to update operation");
        operation.set_state(OperationState::Complete);
        queue
            .update(&operation)
            .expect("Failed to update operation");

        // Try to update terminal operation
        operation.set_state(OperationState::Downloading);
        let result = queue.update(&operation);
        assert!(result.is_err());
        assert!(matches!(
            result.expect_err("Should be error"),
            AgentError::Operation(OperationError::AlreadyCompleted(_))
        ));
    }

    #[test]
    fn test_delete_operation() {
        let queue = create_test_queue();
        let operation = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        queue
            .create(&operation)
            .expect("Failed to create operation");

        let result = queue.delete(operation.operation_id);
        assert!(result.is_ok());

        let result = queue.get(operation.operation_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_list_operations() {
        let queue = create_test_queue();

        let op1 = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        let op2 = Operation::new("wow".to_string(), OperationType::Update, Priority::High);

        // Create first operation
        queue.create(&op1).expect("Failed to create operation");

        // Move first operation to terminal state so we can create second one
        let mut op1_mut = queue
            .get(op1.operation_id)
            .expect("Failed to get operation");
        op1_mut.set_state(OperationState::Initializing);
        queue.update(&op1_mut).expect("Failed to update operation");
        op1_mut.set_state(OperationState::Downloading);
        queue.update(&op1_mut).expect("Failed to update operation");
        op1_mut.set_state(OperationState::Verifying);
        queue.update(&op1_mut).expect("Failed to update operation");
        queue.update(&op1_mut).expect("Failed to update operation");

        op1_mut.set_state(OperationState::Complete);
        queue.update(&op1_mut).expect("Failed to update operation");
        queue.create(&op2).expect("Failed to create operation");

        let operations = queue.list().expect("Failed to list operations");
        assert_eq!(operations.len(), 2);
    }

    #[test]
    fn test_list_by_state() {
        let queue = create_test_queue();

        let mut op1 = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        queue.create(&op1).expect("Failed to create operation");

        let queued = queue
            .list_by_state(OperationState::Queued)
            .expect("Failed to list operations by state");
        assert_eq!(queued.len(), 1);

        op1.set_state(OperationState::Initializing);
        queue.update(&op1).expect("Failed to update operation");

        let queued = queue
            .list_by_state(OperationState::Queued)
            .expect("Failed to list operations by state");
        assert_eq!(queued.len(), 0);

        let initializing = queue
            .list_by_state(OperationState::Initializing)
            .expect("Failed to list operations by state");
        assert_eq!(initializing.len(), 1);
    }

    #[test]
    fn test_list_by_product() {
        let queue = create_test_queue();

        let op1 = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        queue.create(&op1).expect("Failed to create operation");

        let operations = queue
            .list_by_product("wow")
            .expect("Failed to list operations by product");
        assert_eq!(operations.len(), 1);

        let operations = queue
            .list_by_product("wow_classic")
            .expect("Failed to list operations by product");
        assert_eq!(operations.len(), 0);
    }

    #[test]
    fn test_get_active_operation_for_product() {
        let queue = create_test_queue();

        // No active operation initially
        assert!(
            queue
                .get_active_operation_for_product("wow")
                .expect("Operation should succeed")
                .is_none()
        );

        let operation = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        queue
            .create(&operation)
            .expect("Failed to create operation");

        // Should return operation ID
        let op_id = queue
            .get_active_operation_for_product("wow")
            .expect("Failed to get active operation");
        assert_eq!(op_id, Some(operation.operation_id));
    }

    #[test]
    fn test_cleanup_old_operations() {
        let queue = create_test_queue();

        // Insert old completed operation (91 days ago)
        let db = queue.db.lock().expect("Failed to acquire lock");
        db.connection().execute(
            "INSERT INTO operations (operation_id, product_code, operation_type, state, priority, created_at, updated_at, completed_at)
             VALUES ('00000000-0000-0000-0000-000000000001', 'wow', 'Install', 'Complete', 'Normal', 
             (SELECT strftime('%Y-%m-%dT%H:%M:%SZ', datetime('now', '-91 days'))), 
             (SELECT strftime('%Y-%m-%dT%H:%M:%SZ', datetime('now', '-91 days'))), 
             (SELECT strftime('%Y-%m-%dT%H:%M:%SZ', datetime('now', '-91 days'))))",
            [],).expect("Database operation should succeed");

        // Insert recent completed operation (1 day ago)
        db.connection().execute(
            "INSERT INTO operations (operation_id, product_code, operation_type, state, priority, created_at, updated_at, completed_at)
             VALUES ('00000000-0000-0000-0000-000000000002', 'wow', 'Update', 'Complete', 'Normal', 
             (SELECT strftime('%Y-%m-%dT%H:%M:%SZ', datetime('now', '-1 day'))), 
             (SELECT strftime('%Y-%m-%dT%H:%M:%SZ', datetime('now', '-1 day'))), 
             (SELECT strftime('%Y-%m-%dT%H:%M:%SZ', datetime('now', '-1 day'))))",
            [],).expect("Database operation should succeed");
        drop(db);

        // Run cleanup
        let count = queue
            .cleanup_old_operations()
            .expect("Failed to cleanup operations");
        assert_eq!(count, 1);

        // Verify operations
        let operations = queue.list().expect("Failed to list operations");
        assert_eq!(operations.len(), 1);
        assert_eq!(
            operations[0].operation_id,
            Uuid::parse_str("00000000-0000-0000-0000-000000000002").expect("Failed to parse UUID")
        );
    }

    #[test]
    fn test_operation_with_progress() {
        let queue = create_test_queue();
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        let progress = Progress::new("downloading".to_string(), 1000000, 100);
        operation.set_progress(progress);

        queue
            .create(&operation)
            .expect("Failed to create operation");

        let retrieved = queue
            .get(operation.operation_id)
            .expect("Failed to get operation");
        assert!(retrieved.progress.is_some());
        assert_eq!(
            retrieved.progress.expect("Progress should exist").phase,
            "downloading"
        );
    }

    #[test]
    fn test_operation_with_error() {
        let queue = create_test_queue();
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        operation.set_error(
            "DOWNLOAD_FAILED".to_string(),
            "Failed to download archive".to_string(),
            None,
        );

        queue
            .create(&operation)
            .expect("Failed to create operation");

        let retrieved = queue
            .get(operation.operation_id)
            .expect("Failed to get operation");
        assert!(retrieved.error.is_some());
        assert_eq!(
            retrieved.error.expect("Error should exist").code,
            "DOWNLOAD_FAILED"
        );
        assert_eq!(retrieved.state, OperationState::Failed);
    }

    #[test]
    fn test_find_interrupted_operations() {
        let queue = create_test_queue();

        // Add additional test products
        let db_guard = queue.db.lock().expect("Failed to lock database");
        for product_code in ["d3", "hs", "sc2"] {
            db_guard
                .connection()
                .execute(
                    "INSERT INTO products (product_code, name, status, created_at, updated_at)
                     VALUES (?1, ?2, 'Available', datetime('now'), datetime('now'))",
                    [product_code, &format!("{} Product", product_code)],
                )
                .expect("Failed to insert test product");
        }
        drop(db_guard);

        // Create operations in various states
        // op1: Queued (should be found)
        let op1 = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        queue.create(&op1).expect("Failed to create operation");

        // op2: Downloading (should be found)
        let mut op2 = Operation::new("d3".to_string(), OperationType::Install, Priority::Normal);
        queue.create(&op2).expect("Failed to create operation");
        op2.set_state(OperationState::Initializing);
        queue.update(&op2).expect("Failed to update operation");
        op2.set_state(OperationState::Downloading);
        queue.update(&op2).expect("Failed to update operation");

        // op3: Complete (should NOT be found - terminal state)
        let mut op3 = Operation::new("hs".to_string(), OperationType::Install, Priority::Normal);
        queue.create(&op3).expect("Failed to create operation");
        op3.set_state(OperationState::Initializing);
        queue.update(&op3).expect("Failed to update operation");
        op3.set_state(OperationState::Downloading);
        queue.update(&op3).expect("Failed to update operation");
        op3.set_state(OperationState::Verifying);
        queue.update(&op3).expect("Failed to update operation");
        op3.set_state(OperationState::Complete);
        queue.update(&op3).expect("Failed to update operation");

        // op4: Failed (should NOT be found - terminal state)
        let mut op4 = Operation::new("sc2".to_string(), OperationType::Install, Priority::Normal);
        queue.create(&op4).expect("Failed to create operation");
        op4.set_state(OperationState::Initializing);
        queue.update(&op4).expect("Failed to update operation");
        op4.set_error(
            "TEST_ERROR".to_string(),
            "Test error for failed operation".to_string(),
            None,
        );
        op4.set_state(OperationState::Failed);
        queue.update(&op4).expect("Failed to update operation");

        // Find interrupted operations
        let interrupted = queue
            .find_interrupted_operations()
            .expect("Failed to find interrupted operations");

        // Should find op1 (Queued) and op2 (Downloading), but not op3 (Complete) or op4 (Failed)
        assert_eq!(interrupted.len(), 2);

        let product_codes: Vec<_> = interrupted
            .iter()
            .map(|o| o.product_code.as_str())
            .collect();
        assert!(product_codes.contains(&"wow"));
        assert!(product_codes.contains(&"d3"));
        assert!(!product_codes.contains(&"hs"));
        assert!(!product_codes.contains(&"sc2"));
    }
}
