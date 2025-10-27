//! Database schema and migrations for agent service
//!
//! Provides `SQLite` schema initialization, migrations, and connection management.
//! Implements the database schema from data-model.md with proper indices.

// Note: SqliteResult is used in test code for .collect() calls but linter doesn't detect it
#[allow(unused_imports)]
use rusqlite::{Connection, Result as SqliteResult, params};
use std::path::Path;

use crate::error::Result;

/// Database schema version
const SCHEMA_VERSION: i32 = 2;

/// Database connection wrapper with migration support
pub struct Database {
    conn: Connection,
}

impl Database {
    // Future use: T078 (main.rs file-based database)
    #[allow(dead_code)]
    /// Open or create database at the given path
    ///
    /// Automatically initializes schema if database is new or runs migrations
    /// if schema version is outdated.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;

        // Enable foreign key constraints
        conn.execute("PRAGMA foreign_keys = ON", [])?;

        let mut db = Self { conn };
        db.initialize_schema()?;

        Ok(db)
    }

    // Used in tests
    #[allow(dead_code)]
    /// Open in-memory database (for testing)
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute("PRAGMA foreign_keys = ON", [])?;

        let mut db = Self { conn };
        db.initialize_schema()?;

        Ok(db)
    }

    /// Get reference to underlying connection
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    // Future use: Transaction support
    #[allow(dead_code)]
    /// Get mutable reference to underlying connection
    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Initialize database schema
    ///
    /// Checks current schema version and applies migrations if needed.
    fn initialize_schema(&mut self) -> Result<()> {
        let mut current_version = self.get_schema_version()?;

        if current_version == 0 {
            // New database - create initial schema
            self.create_schema_v1()?;
            current_version = 1;
        }

        if current_version < SCHEMA_VERSION {
            // Run migrations to bring schema up to current version
            self.run_migrations(current_version)?;
        }

        Ok(())
    }

    /// Get current schema version
    fn get_schema_version(&self) -> Result<i32> {
        // Check if schema_version table exists
        let table_exists: bool = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_version'",
                [],
                |row| row.get(0),
            )
            .map(|count: i32| count > 0)?;

        if !table_exists {
            return Ok(0);
        }

        // Get version from table (only one row with id=1)
        let version = self
            .conn
            .query_row(
                "SELECT version FROM schema_version WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        Ok(version)
    }

    /// Set schema version
    fn set_schema_version(&mut self, version: i32) -> Result<()> {
        // Create schema_version table if it doesn't exist
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                version INTEGER NOT NULL,
                applied_at TEXT NOT NULL
            )",
            [],
        )?;

        // Replace current version (using INSERT OR REPLACE for upsert)
        self.conn.execute(
            "INSERT OR REPLACE INTO schema_version (id, version, applied_at) VALUES (1, ?1, datetime('now'))",
            params![version],
        )?;

        Ok(())
    }

    /// Create initial schema (version 1)
    fn create_schema_v1(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;

        // Products table (v1 schema)
        tx.execute(
            "CREATE TABLE products (
                product_code TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                status TEXT NOT NULL,
                version TEXT,
                install_path TEXT,
                size_bytes INTEGER,
                region TEXT,
                locale TEXT,
                installation_mode TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,

                CHECK (status IN ('Available', 'Installing', 'Installed', 'Updating', 'Repairing', 'Verifying', 'Uninstalling', 'Corrupted')),
                CHECK (installation_mode IS NULL OR installation_mode IN ('CASC', 'Containerless')),
                CHECK (region IS NULL OR region IN ('us', 'eu', 'kr', 'cn', 'tw')),
                CHECK (size_bytes IS NULL OR size_bytes >= 0)
            )",
            [],
        )?;

        tx.execute("CREATE INDEX idx_products_status ON products(status)", [])?;

        tx.execute(
            "CREATE INDEX idx_products_updated_at ON products(updated_at DESC)",
            [],
        )?;

        // Operations table
        tx.execute(
            "CREATE TABLE operations (
                operation_id TEXT PRIMARY KEY,
                product_code TEXT NOT NULL,
                operation_type TEXT NOT NULL,
                state TEXT NOT NULL,
                priority TEXT NOT NULL DEFAULT 'Normal',
                parameters_json TEXT,
                progress_json TEXT,
                error_json TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                started_at TEXT,
                completed_at TEXT,

                FOREIGN KEY (product_code) REFERENCES products(product_code) ON DELETE CASCADE,

                CHECK (operation_type IN ('Install', 'Update', 'Repair', 'Verify', 'Uninstall')),
                CHECK (state IN ('Queued', 'Initializing', 'Downloading', 'Verifying', 'Complete', 'Failed', 'Cancelled')),
                CHECK (priority IN ('Low', 'Normal', 'High'))
            )",
            [],
        )?;

        tx.execute(
            "CREATE INDEX idx_operations_product_code ON operations(product_code)",
            [],
        )?;

        tx.execute("CREATE INDEX idx_operations_state ON operations(state)", [])?;

        tx.execute(
            "CREATE INDEX idx_operations_created_at ON operations(created_at DESC)",
            [],
        )?;

        tx.execute(
            "CREATE INDEX idx_operations_product_state ON operations(product_code, state)",
            [],
        )?;

        // Versions table (cached from NGDP)
        tx.execute(
            "CREATE TABLE versions (
                product_code TEXT NOT NULL,
                version TEXT NOT NULL,
                build_config TEXT NOT NULL,
                cdn_config TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                release_date TEXT NOT NULL,
                is_latest INTEGER NOT NULL DEFAULT 0,
                cached_at TEXT NOT NULL,

                PRIMARY KEY (product_code, version),
                FOREIGN KEY (product_code) REFERENCES products(product_code) ON DELETE CASCADE,

                CHECK (size_bytes >= 0),
                CHECK (is_latest IN (0, 1))
            )",
            [],
        )?;

        tx.execute(
            "CREATE INDEX idx_versions_product_latest ON versions(product_code, is_latest)",
            [],
        )?;

        tx.execute(
            "CREATE INDEX idx_versions_release_date ON versions(release_date DESC)",
            [],
        )?;

        // Configuration table
        tx.execute(
            "CREATE TABLE configuration (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        tx.commit()?;

        self.set_schema_version(1)?;

        Ok(())
    }

    /// Run migrations from old version to current
    fn run_migrations(&mut self, from_version: i32) -> Result<()> {
        // Apply migrations sequentially
        // Note: Schema v1 must already exist, this only handles incremental migrations

        if from_version < 2 {
            self.migrate_v1_to_v2()?;
        }

        // Future migrations will go here:
        // if from_version < 3 {
        //     self.migrate_v2_to_v3()?;
        // }

        Ok(())
    }

    /// Migrate from schema version 1 to version 2
    ///
    /// Adds update detection fields to products table (T087).
    fn migrate_v1_to_v2(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;

        // Add is_update_available column
        tx.execute(
            "ALTER TABLE products ADD COLUMN is_update_available INTEGER",
            [],
        )?;

        // Add available_version column
        tx.execute("ALTER TABLE products ADD COLUMN available_version TEXT", [])?;

        tx.commit()?;

        self.set_schema_version(2)?;

        Ok(())
    }

    /// Clean up old operations (FR-031: 90-day retention)
    ///
    /// Removes operations older than 90 days in terminal states (Complete, Failed, Cancelled).
    /// Returns the number of operations removed.
    pub fn cleanup_old_operations(&mut self) -> Result<usize> {
        let count = self.conn.execute(
            "DELETE FROM operations
             WHERE state IN ('Complete', 'Failed', 'Cancelled')
             AND datetime(completed_at) < datetime('now', '-90 days')",
            [],
        )?;

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_database() {
        let db = Database::in_memory();
        assert!(db.is_ok());
    }

    #[test]
    fn test_schema_initialization() {
        let db = Database::in_memory().expect("Failed to create test database");

        // Check that all tables exist
        let tables: Vec<String> = db
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .expect("Operation should succeed")
            .query_map([], |row| row.get(0))
            .expect("Operation should succeed")
            .collect::<SqliteResult<Vec<_>>>()
            .expect("Operation should succeed");

        assert!(tables.contains(&"products".to_string()));
        assert!(tables.contains(&"operations".to_string()));
        assert!(tables.contains(&"versions".to_string()));
        assert!(tables.contains(&"configuration".to_string()));
        assert!(tables.contains(&"schema_version".to_string()));
    }

    #[test]
    fn test_schema_version() {
        let db = Database::in_memory().expect("Failed to create test database");

        // Check that columns from migration v2 exist
        let column_check: rusqlite::Result<i32> = db.conn.query_row(
            "SELECT is_update_available FROM products WHERE 1=0",
            [],
            |_row| Ok(0),
        );
        // Should succeed (column exists) even though no rows match
        assert!(matches!(
            column_check,
            Err(rusqlite::Error::QueryReturnedNoRows)
        ));

        let version = db
            .get_schema_version()
            .expect("Failed to get schema version");

        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_foreign_keys_enabled() {
        let db = Database::in_memory().expect("Failed to create test database");

        let foreign_keys: i32 = db
            .conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .expect("Operation should succeed");

        assert_eq!(foreign_keys, 1);
    }

    #[test]
    fn test_products_table_constraints() {
        let db = Database::in_memory().expect("Failed to create test database");

        // Test status constraint
        let result = db.conn.execute(
            "INSERT INTO products (product_code, name, status, created_at, updated_at)
             VALUES ('test', 'Test', 'InvalidStatus', datetime('now'), datetime('now'))",
            [],
        );
        assert!(result.is_err());

        // Test valid status
        let result = db.conn.execute(
            "INSERT INTO products (product_code, name, status, created_at, updated_at)
             VALUES ('test', 'Test', 'Available', datetime('now'), datetime('now'))",
            [],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_operations_table_constraints() {
        let db = Database::in_memory().expect("Failed to create test database");

        // Insert a product first (foreign key requirement)
        db.conn
            .execute(
                "INSERT INTO products (product_code, name, status, created_at, updated_at)
             VALUES ('wow', 'World of Warcraft', 'Available', datetime('now'), datetime('now'))",
                [],
            )
            .expect("Operation should succeed");

        // Test operation_type constraint
        let result = db.conn.execute(
            "INSERT INTO operations (operation_id, product_code, operation_type, state, priority, created_at, updated_at)
             VALUES ('op1', 'wow', 'InvalidType', 'Queued', 'Normal', datetime('now'), datetime('now'))",
            [],
        );
        assert!(result.is_err());

        // Test valid operation
        let result = db.conn.execute(
            "INSERT INTO operations (operation_id, product_code, operation_type, state, priority, created_at, updated_at)
             VALUES ('op1', 'wow', 'Install', 'Queued', 'Normal', datetime('now'), datetime('now'))",
            [],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_foreign_key_cascade() {
        let db = Database::in_memory().expect("Failed to create test database");

        // Insert product and operation
        db.conn
            .execute(
                "INSERT INTO products (product_code, name, status, created_at, updated_at)
             VALUES ('wow', 'World of Warcraft', 'Available', datetime('now'), datetime('now'))",
                [],
            )
            .expect("Operation should succeed");

        db.conn.execute(
            "INSERT INTO operations (operation_id, product_code, operation_type, state, priority, created_at, updated_at)
             VALUES ('op1', 'wow', 'Install', 'Queued', 'Normal', datetime('now'), datetime('now'))",
            [],).expect("Database operation should succeed");

        // Delete product - should cascade to operations
        db.conn
            .execute("DELETE FROM products WHERE product_code = 'wow'", [])
            .expect("Operation should succeed");

        // Check that operation was deleted
        let count: i32 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM operations WHERE operation_id = 'op1'",
                [],
                |row| row.get(0),
            )
            .expect("Operation should succeed");

        assert_eq!(count, 0);
    }

    #[test]
    fn test_versions_table() {
        let db = Database::in_memory().expect("Failed to create test database");

        // Insert product first
        db.conn
            .execute(
                "INSERT INTO products (product_code, name, status, created_at, updated_at)
             VALUES ('wow', 'World of Warcraft', 'Available', datetime('now'), datetime('now'))",
                [],
            )
            .expect("Operation should succeed");

        // Insert version
        let result = db.conn.execute(
            "INSERT INTO versions (product_code, version, build_config, cdn_config, size_bytes, release_date, is_latest, cached_at)
             VALUES ('wow', '10.2.0.52607', 'abc123', 'def456', 50000000000, datetime('now'), 1, datetime('now'))",
            [],
        );
        assert!(result.is_ok());

        // Test size_bytes constraint
        let result = db.conn.execute(
            "INSERT INTO versions (product_code, version, build_config, cdn_config, size_bytes, release_date, is_latest, cached_at)
             VALUES ('wow', '10.2.0.52608', 'abc124', 'def457', -100, datetime('now'), 0, datetime('now'))",
            [],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_configuration_table() {
        let db = Database::in_memory().expect("Failed to create test database");

        // Insert configuration
        let result = db.conn.execute(
            "INSERT INTO configuration (key, value, updated_at)
             VALUES ('bind_address', '127.0.0.1', datetime('now'))",
            [],
        );
        assert!(result.is_ok());

        // Read back
        let value: String = db
            .conn
            .query_row(
                "SELECT value FROM configuration WHERE key = 'bind_address'",
                [],
                |row| row.get(0),
            )
            .expect("Operation should succeed");

        assert_eq!(value, "127.0.0.1");
    }

    #[test]
    fn test_cleanup_old_operations() {
        let mut db = Database::in_memory().expect("Failed to create test database");

        // Insert product
        db.conn
            .execute(
                "INSERT INTO products (product_code, name, status, created_at, updated_at)
             VALUES ('wow', 'World of Warcraft', 'Available', datetime('now'), datetime('now'))",
                [],
            )
            .expect("Operation should succeed");

        // Insert old completed operation (91 days ago)
        db.conn.execute(
            "INSERT INTO operations (operation_id, product_code, operation_type, state, priority, created_at, updated_at, completed_at)
             VALUES ('old_op', 'wow', 'Install', 'Complete', 'Normal', datetime('now', '-91 days'), datetime('now', '-91 days'), datetime('now', '-91 days'))",
            [],).expect("Database operation should succeed");

        // Insert recent completed operation (1 day ago)
        db.conn.execute(
            "INSERT INTO operations (operation_id, product_code, operation_type, state, priority, created_at, updated_at, completed_at)
             VALUES ('recent_op', 'wow', 'Update', 'Complete', 'Normal', datetime('now', '-1 day'), datetime('now', '-1 day'), datetime('now', '-1 day'))",
            [],).expect("Database operation should succeed");

        // Insert active operation
        db.conn.execute(
            "INSERT INTO operations (operation_id, product_code, operation_type, state, priority, created_at, updated_at, started_at)
             VALUES ('active_op', 'wow', 'Repair', 'Downloading', 'Normal', datetime('now'), datetime('now'), datetime('now'))",
            [],).expect("Database operation should succeed");

        // Run cleanup
        let count = db
            .cleanup_old_operations()
            .expect("Failed to cleanup operations");
        assert_eq!(count, 1); // Only the 91-day-old operation should be deleted

        // Verify operations
        let remaining: i32 = db
            .conn
            .query_row("SELECT COUNT(*) FROM operations", [], |row| row.get(0))
            .expect("Operation should succeed");

        assert_eq!(remaining, 2); // recent_op and active_op should remain
    }

    #[test]
    fn test_indices_exist() {
        let db = Database::in_memory().expect("Failed to create test database");

        let indices: Vec<String> = db
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index' ORDER BY name")
            .expect("Operation should succeed")
            .query_map([], |row| row.get(0))
            .expect("Operation should succeed")
            .collect::<SqliteResult<Vec<_>>>()
            .expect("Operation should succeed");

        // Check for expected indices
        assert!(indices.contains(&"idx_products_status".to_string()));
        assert!(indices.contains(&"idx_products_updated_at".to_string()));
        assert!(indices.contains(&"idx_operations_product_code".to_string()));
        assert!(indices.contains(&"idx_operations_state".to_string()));
        assert!(indices.contains(&"idx_operations_created_at".to_string()));
        assert!(indices.contains(&"idx_operations_product_state".to_string()));
        assert!(indices.contains(&"idx_versions_product_latest".to_string()));
        assert!(indices.contains(&"idx_versions_release_date".to_string()));
    }
}
