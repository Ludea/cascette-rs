//! Product registry for state management
//!
//! Provides CRUD operations for products with validation and state management.
//! Enforces business rules like single active operation per product.

use rusqlite::{Row, params};
use std::sync::{Arc, Mutex};

use crate::error::{AgentError, ProductError, Result};
use crate::models::{InstallationMode, Product, ProductStatus};
use crate::state::db::Database;

/// Product registry managing product state and persistence
pub struct ProductRegistry {
    db: Arc<Mutex<Database>>,
}

// Future use: FR-030 (persistent product state), T078 (main.rs)
#[allow(dead_code)]
impl ProductRegistry {
    /// Create a new product registry
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self { db }
    }

    /// Create a new product
    ///
    /// Returns error if `product_code` already exists or validation fails.
    pub fn create(&self, product: &Product) -> Result<()> {
        product
            .validate()
            .map_err(|e| AgentError::Product(ProductError::InvalidCode(e)))?;

        let db = self.db.lock().expect("Failed to acquire lock");
        let conn = db.connection();

        conn.execute(
            "INSERT INTO products (
                product_code, name, status, version, install_path,
                size_bytes, region, locale, installation_mode,
                is_update_available, available_version,
                created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                product.product_code,
                product.name,
                status_to_string(product.status),
                product.version,
                product.install_path,
                product.size_bytes.map(|v| v as i64),
                product.region,
                product.locale,
                product.installation_mode.map(mode_to_string),
                product.is_update_available.map(|v| if v { 1 } else { 0 }),
                product.available_version,
                product.created_at.to_rfc3339(),
                product.updated_at.to_rfc3339(),
            ],
        )
        .map_err(|e| {
            if e.to_string().contains("UNIQUE constraint failed") {
                AgentError::Product(ProductError::InvalidCode(format!(
                    "Product {} already exists",
                    product.product_code
                )))
            } else {
                AgentError::Database(e)
            }
        })?;

        Ok(())
    }

    /// Get a product by `product_code`
    pub fn get(&self, product_code: &str) -> Result<Product> {
        let db = self.db.lock().expect("Failed to acquire lock");
        let conn = db.connection();

        let product = conn
            .query_row(
                "SELECT product_code, name, status, version, install_path,
                    size_bytes, region, locale, installation_mode,
                    is_update_available, available_version,
                    created_at, updated_at
             FROM products
             WHERE product_code = ?1",
                params![product_code],
                parse_product_row,
            )
            .map_err(|e| {
                if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
                    AgentError::Product(ProductError::NotFound(product_code.to_string()))
                } else {
                    AgentError::Database(e)
                }
            })?;

        Ok(product)
    }

    /// Update an existing product
    ///
    /// Validates state transition and data consistency.
    pub fn update(&self, product: &Product) -> Result<()> {
        product
            .validate()
            .map_err(|e| AgentError::Product(ProductError::InvalidCode(e)))?;

        // Get current product to validate state transition
        let current = self.get(&product.product_code)?;
        if !current.can_transition_to(product.status) {
            return Err(AgentError::Product(ProductError::InvalidVersion(format!(
                "Invalid state transition from {:?} to {:?}",
                current.status, product.status
            ))));
        }

        let db = self.db.lock().expect("Failed to acquire lock");
        let conn = db.connection();

        let rows_affected = conn.execute(
            "UPDATE products
             SET name = ?2, status = ?3, version = ?4, install_path = ?5,
                 size_bytes = ?6, region = ?7, locale = ?8, installation_mode = ?9,
                 is_update_available = ?10, available_version = ?11,
                 updated_at = ?12
             WHERE product_code = ?1",
            params![
                product.product_code,
                product.name,
                status_to_string(product.status),
                product.version,
                product.install_path,
                product.size_bytes.map(|v| v as i64),
                product.region,
                product.locale,
                product.installation_mode.map(mode_to_string),
                product.is_update_available.map(|v| if v { 1 } else { 0 }),
                product.available_version,
                product.updated_at.to_rfc3339(),
            ],
        )?;

        if rows_affected == 0 {
            return Err(AgentError::Product(ProductError::NotFound(
                product.product_code.clone(),
            )));
        }

        Ok(())
    }

    /// Delete a product
    pub fn delete(&self, product_code: &str) -> Result<()> {
        let db = self.db.lock().expect("Failed to acquire lock");
        let conn = db.connection();

        let rows_affected = conn.execute(
            "DELETE FROM products WHERE product_code = ?1",
            params![product_code],
        )?;

        if rows_affected == 0 {
            return Err(AgentError::Product(ProductError::NotFound(
                product_code.to_string(),
            )));
        }

        Ok(())
    }

    /// List all products
    pub fn list(&self) -> Result<Vec<Product>> {
        let db = self.db.lock().expect("Failed to acquire lock");
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT product_code, name, status, version, install_path,
                    size_bytes, region, locale, installation_mode,
                    is_update_available, available_version,
                    created_at, updated_at
             FROM products
             ORDER BY updated_at DESC",
        )?;

        let products = stmt
            .query_map([], parse_product_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(products)
    }

    /// List products by status
    pub fn list_by_status(&self, status: ProductStatus) -> Result<Vec<Product>> {
        let db = self.db.lock().expect("Failed to acquire lock");
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT product_code, name, status, version, install_path,
                    size_bytes, region, locale, installation_mode,
                    is_update_available, available_version,
                    created_at, updated_at
             FROM products
             WHERE status = ?1
             ORDER BY updated_at DESC",
        )?;

        let products = stmt
            .query_map(params![status_to_string(status)], |row| {
                parse_product_row(row)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(products)
    }

    /// Check if a product has an active operation (FR-022)
    ///
    /// Returns true if there is a non-terminal operation for the product.
    pub fn has_active_operation(&self, product_code: &str) -> Result<bool> {
        let db = self.db.lock().expect("Failed to acquire lock");
        let conn = db.connection();

        let count: i32 = conn.query_row(
            "SELECT COUNT(*) FROM operations
             WHERE product_code = ?1
             AND state IN ('Queued', 'Initializing', 'Downloading', 'Verifying')",
            params![product_code],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }

    /// Get the active operation ID for a product, if any
    pub fn get_active_operation_id(&self, product_code: &str) -> Result<Option<String>> {
        let db = self.db.lock().expect("Failed to acquire lock");
        let conn = db.connection();

        let result = conn.query_row(
            "SELECT operation_id FROM operations
             WHERE product_code = ?1
             AND state IN ('Queued', 'Initializing', 'Downloading', 'Verifying')
             ORDER BY created_at DESC
             LIMIT 1",
            params![product_code],
            |row| row.get::<_, String>(0),
        );

        match result {
            Ok(operation_id) => Ok(Some(operation_id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AgentError::Database(e)),
        }
    }

    /// Check for available updates for a product (T085)
    ///
    /// Queries Ribbit service for the latest version and updates the product's
    /// `is_update_available` and `available_version` fields.
    /// Includes tracing spans for observability (T088).
    ///
    /// # Arguments
    ///
    /// * `product_code` - Product to check for updates
    /// * `ribbit_url` - Ribbit TCP service URL (e.g., "us.version.battle.net:1119")
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if update is available, `Ok(false)` if already up-to-date,
    /// or an error if the check failed.
    pub async fn check_for_updates(&self, product_code: &str, ribbit_url: &str) -> Result<bool> {
        use cascette_protocol::client::RibbitClient;
        use tracing::{debug, error, info, instrument, warn};

        // T088: Create tracing span for version check operation
        #[instrument(name = "version_check", fields(product_code = %product_code))]
        async fn check_version_inner(
            product_code: &str,
            ribbit_url: &str,
            current_version: Option<String>,
        ) -> Result<(bool, Option<String>)> {
            debug!("Checking for updates via Ribbit: {}", ribbit_url);

            // Query Ribbit for latest version
            let client = RibbitClient::new(ribbit_url).map_err(|e| {
                error!("Failed to create Ribbit client: {}", e);
                AgentError::Other(format!("Ribbit client error: {}", e))
            })?;

            let endpoint = format!("v1/products/{}/versions", product_code);
            let doc = client.query(&endpoint).await.map_err(|e| {
                error!("Ribbit query failed for {}: {}", product_code, e);
                AgentError::Other(format!("Ribbit query error: {}", e))
            })?;

            // Parse latest version from BPSV document
            // The versions document has columns like: seqn, region, buildconfig, cdnconfig, keyring, buildid, versionsname, productconfig
            let buildid_index = doc.schema().get_field_index("buildid").ok_or_else(|| {
                error!("Missing 'buildid' column in Ribbit response");
                AgentError::Other("Invalid Ribbit response: missing buildid column".to_string())
            })?;

            let versionsname_index =
                doc.schema()
                    .get_field_index("versionsname")
                    .ok_or_else(|| {
                        error!("Missing 'versionsname' column in Ribbit response");
                        AgentError::Other(
                            "Invalid Ribbit response: missing versionsname column".to_string(),
                        )
                    })?;

            // Get the latest version (first row)
            let latest_row = doc.rows().first().ok_or_else(|| {
                error!("No versions found in Ribbit response");
                AgentError::Other("No versions available for product".to_string())
            })?;

            let latest_buildid = latest_row.get_raw(buildid_index).ok_or_else(|| {
                error!("Invalid buildid index in Ribbit response");
                AgentError::Other(
                    "Invalid Ribbit response: buildid index out of bounds".to_string(),
                )
            })?;
            let latest_version = latest_row.get_raw(versionsname_index).ok_or_else(|| {
                error!("Invalid versionsname index in Ribbit response");
                AgentError::Other(
                    "Invalid Ribbit response: versionsname index out of bounds".to_string(),
                )
            })?;

            info!(
                "Latest version: {} (build {})",
                latest_version, latest_buildid
            );

            // Compare with current version
            let update_available = if let Some(current) = current_version.as_ref() {
                if current != latest_version {
                    info!("Update available: {} -> {}", current, latest_version);
                    true
                } else {
                    debug!("Already up-to-date: {}", current);
                    false
                }
            } else {
                warn!("No current version, marking update as available");
                true
            };

            Ok((update_available, Some(latest_version.to_string())))
        }

        // Get current product
        let product = self.get(product_code)?;

        // Check version with tracing
        let (update_available, available_version) =
            check_version_inner(product_code, ribbit_url, product.version.clone()).await?;

        // Update product with version check results
        let mut updated_product = product;
        updated_product.is_update_available = Some(update_available);
        updated_product.available_version = available_version;
        updated_product.updated_at = chrono::Utc::now();

        self.update(&updated_product)?;

        Ok(update_available)
    }

    // Wrapper methods for API compatibility
    // Future use: Product lookup in handlers

    #[allow(dead_code)]
    /// Alias for `get()` - for API compatibility
    pub fn get_product(&self, product_code: &str) -> Result<Product> {
        self.get(product_code)
    }
}

/// Parse a product from a database row
fn parse_product_row(row: &Row) -> rusqlite::Result<Product> {
    Ok(Product {
        product_code: row.get(0)?,
        name: row.get(1)?,
        status: string_to_status(&row.get::<_, String>(2)?).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
            )
        })?,
        version: row.get(3)?,
        install_path: row.get(4)?,
        size_bytes: row.get::<_, Option<i64>>(5)?.map(|v| v as u64),
        region: row.get(6)?,
        locale: row.get(7)?,
        installation_mode: row
            .get::<_, Option<String>>(8)?
            .map(|s| string_to_mode(&s))
            .transpose()
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    8,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                )
            })?,
        is_update_available: row.get::<_, Option<i32>>(9)?.map(|v| v != 0),
        available_version: row.get(10)?,
        created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(11)?)
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    11,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?
            .with_timezone(&chrono::Utc),
        updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(12)?)
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    12,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?
            .with_timezone(&chrono::Utc),
    })
}

/// Convert `ProductStatus` to database string
// Future use: Database serialization
#[allow(dead_code)]
fn status_to_string(status: ProductStatus) -> &'static str {
    match status {
        ProductStatus::Available => "Available",
        ProductStatus::Installing => "Installing",
        ProductStatus::Installed => "Installed",
        ProductStatus::Updating => "Updating",
        ProductStatus::Repairing => "Repairing",
        ProductStatus::Verifying => "Verifying",
        ProductStatus::Uninstalling => "Uninstalling",
        ProductStatus::Corrupted => "Corrupted",
    }
}

/// Convert database string to `ProductStatus`
fn string_to_status(s: &str) -> std::result::Result<ProductStatus, String> {
    match s {
        "Available" => Ok(ProductStatus::Available),
        "Installing" => Ok(ProductStatus::Installing),
        "Installed" => Ok(ProductStatus::Installed),
        "Updating" => Ok(ProductStatus::Updating),
        "Repairing" => Ok(ProductStatus::Repairing),
        "Verifying" => Ok(ProductStatus::Verifying),
        "Uninstalling" => Ok(ProductStatus::Uninstalling),
        "Corrupted" => Ok(ProductStatus::Corrupted),
        _ => Err(format!("Invalid status: {s}")),
    }
}

/// Convert `InstallationMode` to database string
// Future use: Database serialization
#[allow(dead_code)]
fn mode_to_string(mode: InstallationMode) -> &'static str {
    match mode {
        InstallationMode::Casc => "CASC",
        InstallationMode::Containerless => "Containerless",
    }
}

/// Convert database string to `InstallationMode`
fn string_to_mode(s: &str) -> std::result::Result<InstallationMode, String> {
    match s {
        "CASC" => Ok(InstallationMode::Casc),
        "Containerless" => Ok(InstallationMode::Containerless),
        _ => Err(format!("Invalid installation mode: {s}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_registry() -> ProductRegistry {
        let db = Database::in_memory().expect("Failed to create test database");
        ProductRegistry::new(Arc::new(Mutex::new(db)))
    }

    #[test]
    fn test_create_product() {
        let registry = create_test_registry();
        let product = Product::new("wow".to_string(), "World of Warcraft".to_string());

        let result = registry.create(&product);
        assert!(result.is_ok());

        // Try to create duplicate
        let result = registry.create(&product);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_product() {
        let registry = create_test_registry();
        let product = Product::new("wow".to_string(), "World of Warcraft".to_string());

        registry.create(&product).expect("Failed to create product");

        let retrieved = registry.get("wow").expect("Failed to get product");
        assert_eq!(retrieved.product_code, "wow");
        assert_eq!(retrieved.name, "World of Warcraft");
        assert_eq!(retrieved.status, ProductStatus::Available);
    }

    #[test]
    fn test_get_nonexistent_product() {
        let registry = create_test_registry();

        let result = registry.get("nonexistent");
        assert!(result.is_err());
        assert!(matches!(
            result.expect_err("Should be error"),
            AgentError::Product(ProductError::NotFound(_))
        ));
    }

    #[test]
    fn test_update_product() {
        let registry = create_test_registry();
        let mut product = Product::new("wow".to_string(), "World of Warcraft".to_string());

        registry.create(&product).expect("Failed to create product");

        // Update to Installing
        product.set_status(ProductStatus::Installing);
        let result = registry.update(&product);
        assert!(result.is_ok());

        let retrieved = registry.get("wow").expect("Failed to get product");
        assert_eq!(retrieved.status, ProductStatus::Installing);
    }

    #[test]
    fn test_update_invalid_transition() {
        let registry = create_test_registry();
        let mut product = Product::new("wow".to_string(), "World of Warcraft".to_string());

        registry.create(&product).expect("Failed to create product");

        // Try invalid transition: Available → Installed (must go through Installing)
        product.set_status(ProductStatus::Installed);
        let result = registry.update(&product);
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_product() {
        let registry = create_test_registry();
        let product = Product::new("wow".to_string(), "World of Warcraft".to_string());

        registry.create(&product).expect("Failed to create product");

        let result = registry.delete("wow");
        assert!(result.is_ok());

        let result = registry.get("wow");
        assert!(result.is_err());
    }

    #[test]
    fn test_list_products() {
        let registry = create_test_registry();

        let product1 = Product::new("wow".to_string(), "World of Warcraft".to_string());
        let product2 = Product::new("wow_classic".to_string(), "WoW Classic".to_string());

        registry
            .create(&product1)
            .expect("Failed to create product");
        registry
            .create(&product2)
            .expect("Failed to create product");

        let products = registry.list().expect("Failed to list products");
        assert_eq!(products.len(), 2);
    }

    #[test]
    fn test_list_by_status() {
        let registry = create_test_registry();

        let mut product1 = Product::new("wow".to_string(), "World of Warcraft".to_string());
        let product2 = Product::new("wow_classic".to_string(), "WoW Classic".to_string());

        registry
            .create(&product1)
            .expect("Failed to create product");
        registry
            .create(&product2)
            .expect("Failed to create product");

        // Update product1 to Installing
        product1.set_status(ProductStatus::Installing);
        registry
            .update(&product1)
            .expect("Failed to update product");

        let available = registry
            .list_by_status(ProductStatus::Available)
            .expect("Failed to list by status");
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].product_code, "wow_classic");

        let installing = registry
            .list_by_status(ProductStatus::Installing)
            .expect("Failed to list by status");
        assert_eq!(installing.len(), 1);
        assert_eq!(installing[0].product_code, "wow");
    }

    #[test]
    fn test_has_active_operation() {
        let registry = create_test_registry();
        let product = Product::new("wow".to_string(), "World of Warcraft".to_string());

        registry.create(&product).expect("Failed to create product");

        // No active operation initially
        assert!(
            !registry
                .has_active_operation("wow")
                .expect("Failed to check active operation")
        );

        // Insert an active operation
        let db = registry.db.lock().expect("Failed to acquire lock");
        db.connection().execute(
            "INSERT INTO operations (operation_id, product_code, operation_type, state, priority, created_at, updated_at)
             VALUES ('op1', 'wow', 'Install', 'Queued', 'Normal', datetime('now'), datetime('now'))",
            [],).expect("Database operation should succeed");
        drop(db);

        // Now should have active operation
        assert!(
            registry
                .has_active_operation("wow")
                .expect("Failed to check active operation")
        );
    }

    #[test]
    fn test_get_active_operation_id() {
        let registry = create_test_registry();
        let product = Product::new("wow".to_string(), "World of Warcraft".to_string());

        registry.create(&product).expect("Failed to create product");

        // No active operation initially
        assert!(
            registry
                .get_active_operation_id("wow")
                .expect("Failed to get active operation ID")
                .is_none()
        );

        // Insert an active operation
        let db = registry.db.lock().expect("Failed to acquire lock");
        db.connection().execute(
            "INSERT INTO operations (operation_id, product_code, operation_type, state, priority, created_at, updated_at)
             VALUES ('op1', 'wow', 'Install', 'Downloading', 'Normal', datetime('now'), datetime('now'))",
            [],).expect("Database operation should succeed");
        drop(db);

        // Should return operation ID
        let op_id = registry
            .get_active_operation_id("wow")
            .expect("Failed to get active operation ID");
        assert_eq!(op_id, Some("op1".to_string()));
    }

    #[test]
    fn test_installed_product_with_full_data() {
        let registry = create_test_registry();
        let mut product = Product::new("wow".to_string(), "World of Warcraft".to_string());

        product.set_status(ProductStatus::Installing);
        registry.create(&product).expect("Failed to create product");

        // Set up full installation data
        product.set_status(ProductStatus::Installed);
        product.version = Some("10.2.0.52607".to_string());
        product.install_path = Some("/games/wow".to_string());
        product.size_bytes = Some(50000000000);
        product.region = Some("us".to_string());
        product.locale = Some("enUS".to_string());
        product.installation_mode = Some(InstallationMode::Casc);

        registry.update(&product).expect("Failed to update product");

        let retrieved = registry.get("wow").expect("Failed to get product");
        assert_eq!(retrieved.status, ProductStatus::Installed);
        assert_eq!(retrieved.version, Some("10.2.0.52607".to_string()));
        assert_eq!(retrieved.install_path, Some("/games/wow".to_string()));
        assert_eq!(retrieved.size_bytes, Some(50000000000));
        assert_eq!(retrieved.region, Some("us".to_string()));
        assert_eq!(retrieved.locale, Some("enUS".to_string()));
        assert_eq!(retrieved.installation_mode, Some(InstallationMode::Casc));
    }
}
