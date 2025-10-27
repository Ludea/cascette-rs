//! Product endpoint handlers
//!
//! Provides REST API for product management including install, update, and status.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use std::sync::Arc;

use crate::error::{AgentError, OperationError, ProductError, Result};
use crate::models::ProductStatus;
use crate::models::{Operation, OperationType, Priority};
use crate::server::models::{InstallRequest, OperationResponse, ProductResponse, UpdateRequest};
use crate::state::AppState;

/// POST /`products/{product_code}/install` - Install product
///
/// Creates a new installation operation for the specified product.
/// Validates request and checks for concurrent operations (FR-022).
///
/// # Request Body
///
/// ```json
/// {
///   "build_id": 56313,
///   "install_path": "/games/wow",
///   "region": "us",
///   "locale": "enUS",
///   "tags": ["Windows"],
///   "mode": "casc"
/// }
/// ```
///
/// # Returns
///
/// - 202 Accepted: Operation created and queued
/// - 400 Bad Request: Invalid request parameters
/// - 409 Conflict: Another operation already in progress for this product
pub async fn install_product(
    State(state): State<Arc<AppState>>,
    Path(product_code): Path<String>,
    Json(request): Json<InstallRequest>,
) -> Result<(StatusCode, Json<OperationResponse>)> {
    // Validate product code
    validate_product_code(&product_code)?;

    // Validate request
    validate_install_request(&request)?;

    // Ensure product exists in registry - auto-register if needed
    // This satisfies the foreign key constraint on operations table
    if state.registry.get(&product_code).is_err() {
        // Auto-register new product
        use crate::models::Product;
        let new_product = Product::new(
            product_code.clone(),
            format!("Product {}", product_code), // Generic name, will be updated during install
        );
        state.registry.create(&new_product)?;

        tracing::info!(
            product_code = %product_code,
            "Auto-registered new product"
        );
    }

    // Check for version downgrade (FR-032, T049)
    if let Some(build_id) = request.build_id {
        if let Ok(product) = state.registry.get(&product_code) {
            // If product is installed, check for downgrade
            if let Some(installed_version) = &product.version {
                // Parse installed version as build_id (stored as string)
                if let Ok(installed_build_id) = installed_version.parse::<u32>() {
                    if build_id < installed_build_id {
                        return Err(AgentError::Product(ProductError::DowngradeRejected {
                            product: product_code,
                            current: installed_version.clone(),
                            target: build_id.to_string(),
                        }));
                    }
                }
            }
        }
    }

    // Check for concurrent operations (FR-022)
    if let Some(active_op) = state
        .queue
        .get_active_operation_for_product(&product_code)?
    {
        return Err(AgentError::Operation(OperationError::Conflict {
            product: product_code,
            operation_id: active_op.to_string(),
        }));
    }

    // Create operation with install parameters
    let mut operation = Operation::new(
        product_code.clone(),
        OperationType::Install,
        Priority::Normal,
    );

    // Store install request parameters for executor
    operation.parameters = Some(serde_json::to_value(&request)?);

    // Store operation in database
    state.queue.create_operation(&operation)?;

    // Emit metrics
    state.metrics.record_operation_start("install");

    // Log structured event
    tracing::info!(
        operation_id = %operation.operation_id,
        product_code = %product_code,
        install_path = %request.install_path,
        region = %request.region,
        locale = %request.locale,
        "Installation operation created"
    );

    // TODO: Spawn executor task to process the operation
    // For now, we just return the queued operation

    Ok((
        StatusCode::ACCEPTED,
        Json(OperationResponse::from(operation)),
    ))
}

/// GET /`products/{product_code`} - Get product details
///
/// Returns information about a product including installation status
/// and current operation if any.
///
/// # Returns
///
/// - 200 OK: Product information returned
/// - 404 Not Found: Product does not exist
pub async fn get_product(
    State(state): State<Arc<AppState>>,
    Path(product_code): Path<String>,
) -> Result<Json<ProductResponse>> {
    // Try to get product from registry
    let product = state.registry.get_product(&product_code)?;

    Ok(Json(ProductResponse::from(product)))
}

/// POST /agent/update/{product_code} - Update an installed product (T089)
///
/// Initiates an update operation for an installed product to a newer version.
/// Implements FR-032 (version downgrade prevention) and FR-022 (concurrent operation checks).
///
/// # Validations
///
/// - Product code format (T090)
/// - Product must be installed (T090)
/// - Target version must be newer than current version unless force=true (T091, FR-032)
/// - No concurrent operations on the same product (T092, FR-022)
///
/// # Response
///
/// - 202 Accepted: Operation queued successfully
/// - 400 Bad Request: Invalid product code or request
/// - 404 Not Found: Product not registered
/// - 409 Conflict: Concurrent operation or version downgrade attempt
pub async fn update_product(
    State(state): State<Arc<AppState>>,
    Path(product_code): Path<String>,
    Json(request): Json<UpdateRequest>,
) -> Result<(StatusCode, Json<OperationResponse>)> {
    tracing::info!(
        product_code = %product_code,
        build_id = ?request.build_id,
        force = request.force,
        "Received update request"
    );

    // Validate product code (T089, T090)
    validate_product_code(&product_code)?;

    // Check if product exists and is installed (T090)
    let product = state.registry.get(&product_code)?;

    // Validate product is installed (T090)
    if product.status != ProductStatus::Installed {
        return Err(AgentError::Product(ProductError::NotInstalled(
            product_code,
        )));
    }

    // Check for version downgrade (T091, FR-032)
    if !request.force {
        if let (Some(current_version), Some(build_id)) = (&product.version, request.build_id) {
            // Parse current version as build_id
            if let Ok(current_build_id) = current_version.parse::<u32>() {
                if build_id < current_build_id {
                    return Err(AgentError::Product(ProductError::DowngradeRejected {
                        product: product_code,
                        current: current_version.clone(),
                        target: build_id.to_string(),
                    }));
                }
            }
        }
    }

    // Check for concurrent operations (T092, FR-022)
    if let Some(active_op) = state
        .queue
        .get_active_operation_for_product(&product_code)?
    {
        return Err(AgentError::Operation(OperationError::Conflict {
            product: product_code,
            operation_id: active_op.to_string(),
        }));
    }

    // Create operation with update parameters (T089, T093)
    let mut operation = Operation::new(
        product_code.clone(),
        OperationType::Update,
        Priority::Normal,
    );

    // Build comprehensive parameters for executor (T093)
    // Executor needs: install_path, current_build_id, target_build_id
    let current_build_id = product.version.as_ref().and_then(|v| v.parse::<u32>().ok());

    let target_build_id = request.build_id.or_else(|| {
        // If no specific build_id requested, use available_version from version check
        product
            .available_version
            .as_ref()
            .and_then(|v| v.parse::<u32>().ok())
    });

    // Construct executor parameters
    let executor_params = serde_json::json!({
        "build_id": target_build_id,
        "force": request.force,
        "install_path": product.install_path.as_ref().ok_or_else(|| {
            AgentError::Product(ProductError::NotInstalled(product_code.clone()))
        })?,
        "current_build_id": current_build_id,
        "target_build_id": target_build_id,
    });

    operation.parameters = Some(executor_params);

    // Store operation in database
    state.queue.create_operation(&operation)?;

    // Emit metrics
    state.metrics.record_operation_start("update");

    // Log structured event
    tracing::info!(
        operation_id = %operation.operation_id,
        product_code = %product_code,
        build_id = ?request.build_id,
        force = request.force,
        "Update operation created"
    );

    Ok((
        StatusCode::ACCEPTED,
        Json(OperationResponse::from(operation)),
    ))
}

/// GET /agent/products - List all products
///
/// Returns a list of all products in the registry.
/// Implements T084 for US2 (Update Product).
///
/// # Returns
///
/// - 200 OK: Product list returned successfully
///
/// # Response Format
///
/// ```json
/// {
///   "products": [
///     {
///       "product_code": "wow",
///       "name": "World of Warcraft",
///       "status": "installed",
///       "install_path": "/games/wow",
///       "version": "11.0.7.56313",
///       "_links": {
///         "self": { "href": "/products/wow", "type": "application/json" }
///       }
///     }
///   ],
///   "_links": {
///     "self": { "href": "/agent/products", "type": "application/json" }
///   }
/// }
/// ```
///
/// # Examples
///
/// ```bash
/// curl http://localhost:1120/agent/products
/// ```
pub async fn list_products(
    State(state): State<Arc<AppState>>,
) -> Result<Json<crate::server::models::ProductListResponse>> {
    use crate::server::models::{Link, ProductListResponse};

    // Get all products from registry
    let products = state.registry.list()?;

    // Convert to response models
    let product_responses: Vec<ProductResponse> =
        products.into_iter().map(ProductResponse::from).collect();

    // Create HATEOAS links
    let mut links = std::collections::HashMap::new();
    links.insert(
        "self".to_string(),
        Link {
            href: "/agent/products".to_string(),
            type_: Some("application/json".to_string()),
        },
    );

    let response = ProductListResponse {
        products: product_responses,
        links,
    };

    Ok(Json(response))
}

/// Validate product code format
///
/// Product codes must be alphanumeric with optional underscores and hyphens.
fn validate_product_code(code: &str) -> Result<()> {
    if code.is_empty() {
        return Err(AgentError::Product(ProductError::InvalidCode(
            "Product code cannot be empty".to_string(),
        )));
    }

    if !code
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err(AgentError::Product(ProductError::InvalidCode(format!(
            "Product code '{code}' contains invalid characters"
        ))));
    }

    Ok(())
}

/// Validate install request parameters
fn validate_install_request(request: &InstallRequest) -> Result<()> {
    // Validate install path
    if request.install_path.is_empty() {
        return Err(AgentError::Operation(OperationError::InvalidRequest(
            "install_path cannot be empty".to_string(),
        )));
    }

    // Validate region
    if !is_valid_region(&request.region) {
        return Err(AgentError::Operation(OperationError::InvalidRequest(
            format!("Invalid region: {}", request.region),
        )));
    }

    // Validate locale
    if !is_valid_locale(&request.locale) {
        return Err(AgentError::Operation(OperationError::InvalidRequest(
            format!("Invalid locale: {}", request.locale),
        )));
    }

    Ok(())
}

/// Check if region code is valid
fn is_valid_region(region: &str) -> bool {
    matches!(region, "us" | "eu" | "kr" | "cn" | "tw")
}

/// Check if locale code is valid
///
/// Validates common Blizzard locale codes.
fn is_valid_locale(locale: &str) -> bool {
    matches!(
        locale,
        "enUS"
            | "enGB"
            | "deDE"
            | "frFR"
            | "esES"
            | "esMX"
            | "ptBR"
            | "ruRU"
            | "koKR"
            | "zhCN"
            | "zhTW"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::InstallationMode;
    use crate::observability::Metrics;
    use crate::state::{OperationQueue, ProductRegistry, db::Database};
    use std::sync::{Arc, Mutex};

    async fn setup_test_state() -> Arc<AppState> {
        let db = Arc::new(Mutex::new(
            Database::in_memory().expect("Failed to create test database"),
        ));
        let state = Arc::new(AppState {
            queue: Arc::new(OperationQueue::new(db.clone())),
            registry: Arc::new(ProductRegistry::new(db)),
            metrics: Arc::new(Metrics::new()),
        });

        // Create test product to satisfy foreign key constraints
        use crate::models::Product;
        let product = Product::new("wow".to_string(), "World of Warcraft".to_string());
        let _ = state.registry.create(&product);

        state
    }

    #[tokio::test]
    async fn test_install_product_success() {
        let state = setup_test_state().await;

        let request = InstallRequest {
            build_id: Some(56313),
            install_path: "/games/wow".to_string(),
            region: "us".to_string(),
            locale: "enUS".to_string(),
            tags: vec![],
            mode: InstallationMode::Casc,
        };

        let result = install_product(State(state.clone()), Path("wow".to_string()), Json(request))
            .await
            .expect("Operation should succeed");

        assert_eq!(result.0, StatusCode::ACCEPTED);
        assert_eq!(result.1.0.product_code, "wow");
        assert_eq!(result.1.0.operation_type, OperationType::Install);
        assert_eq!(result.1.0.state, crate::models::OperationState::Queued);

        // Verify operation was created in database
        let operations = state
            .queue
            .list_operations()
            .expect("Failed to list operations");
        let wow_operations: Vec<_> = operations
            .iter()
            .filter(|op| op.product_code == "wow")
            .collect();
        assert_eq!(wow_operations.len(), 1);
    }

    #[tokio::test]
    async fn test_install_product_concurrent_conflict() {
        let state = setup_test_state().await;

        let request = InstallRequest {
            build_id: Some(56313),
            install_path: "/games/wow".to_string(),
            region: "us".to_string(),
            locale: "enUS".to_string(),
            tags: vec![],
            mode: InstallationMode::Casc,
        };

        // First request succeeds
        let _ = install_product(
            State(state.clone()),
            Path("wow".to_string()),
            Json(request.clone()),
        )
        .await
        .expect("Operation should succeed");

        // Second request conflicts
        let result = install_product(State(state), Path("wow".to_string()), Json(request)).await;

        assert!(result.is_err());
        assert!(matches!(
            result.expect_err("Should be error"),
            AgentError::Operation(OperationError::Conflict { .. })
        ));
    }

    #[tokio::test]
    async fn test_install_product_invalid_code() {
        let state = setup_test_state().await;

        let request = InstallRequest {
            build_id: None,
            install_path: "/games/wow".to_string(),
            region: "us".to_string(),
            locale: "enUS".to_string(),
            tags: vec![],
            mode: InstallationMode::Casc,
        };

        let result = install_product(
            State(state),
            Path("wow@invalid!".to_string()),
            Json(request),
        )
        .await;

        assert!(result.is_err());
        assert!(matches!(
            result.expect_err("Should be error"),
            AgentError::Product(ProductError::InvalidCode(_))
        ));
    }

    #[tokio::test]
    async fn test_install_product_empty_path() {
        let state = setup_test_state().await;

        let request = InstallRequest {
            build_id: None,
            install_path: "".to_string(),
            region: "us".to_string(),
            locale: "enUS".to_string(),
            tags: vec![],
            mode: InstallationMode::Casc,
        };

        let result = install_product(State(state), Path("wow".to_string()), Json(request)).await;

        assert!(result.is_err());
        assert!(matches!(
            result.expect_err("Should be error"),
            AgentError::Operation(OperationError::InvalidRequest(_))
        ));
    }

    #[tokio::test]
    async fn test_install_product_invalid_region() {
        let state = setup_test_state().await;

        let request = InstallRequest {
            build_id: None,
            install_path: "/games/wow".to_string(),
            region: "invalid".to_string(),
            locale: "enUS".to_string(),
            tags: vec![],
            mode: InstallationMode::Casc,
        };

        let result = install_product(State(state), Path("wow".to_string()), Json(request)).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_install_product_invalid_locale() {
        let state = setup_test_state().await;

        let request = InstallRequest {
            build_id: None,
            install_path: "/games/wow".to_string(),
            region: "us".to_string(),
            locale: "invalid".to_string(),
            tags: vec![],
            mode: InstallationMode::Casc,
        };

        let result = install_product(State(state), Path("wow".to_string()), Json(request)).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_install_product_downgrade_rejected() {
        let state = setup_test_state().await;

        // First install version 200
        let mut product = state
            .registry
            .get_product("wow")
            .expect("Product should exist");
        product.version = Some("200".to_string());
        state
            .registry
            .update(&product)
            .expect("Failed to update product");

        // Try to install version 100 (downgrade)
        let request = InstallRequest {
            build_id: Some(100),
            install_path: "/games/wow".to_string(),
            region: "us".to_string(),
            locale: "enUS".to_string(),
            tags: vec![],
            mode: InstallationMode::Casc,
        };

        let result = install_product(State(state), Path("wow".to_string()), Json(request)).await;

        assert!(result.is_err());
        assert!(matches!(
            result.expect_err("Should be error"),
            AgentError::Product(ProductError::DowngradeRejected { .. })
        ));
    }

    #[tokio::test]
    async fn test_install_product_same_version_allowed() {
        let state = setup_test_state().await;

        // First install version 200
        let mut product = state
            .registry
            .get_product("wow")
            .expect("Product should exist");
        product.version = Some("200".to_string());
        state
            .registry
            .update(&product)
            .expect("Failed to update product");

        // Install same version 200 (should be allowed)
        let request = InstallRequest {
            build_id: Some(200),
            install_path: "/games/wow".to_string(),
            region: "us".to_string(),
            locale: "enUS".to_string(),
            tags: vec![],
            mode: InstallationMode::Casc,
        };

        let result = install_product(State(state), Path("wow".to_string()), Json(request)).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_install_product_upgrade_allowed() {
        let state = setup_test_state().await;

        // First install version 200
        let mut product = state
            .registry
            .get_product("wow")
            .expect("Product should exist");
        product.version = Some("200".to_string());
        state
            .registry
            .update(&product)
            .expect("Failed to update product");

        // Install version 300 (upgrade - should be allowed)
        let request = InstallRequest {
            build_id: Some(300),
            install_path: "/games/wow".to_string(),
            region: "us".to_string(),
            locale: "enUS".to_string(),
            tags: vec![],
            mode: InstallationMode::Casc,
        };

        let result = install_product(State(state), Path("wow".to_string()), Json(request)).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_install_product_fresh_install_no_version_check() {
        let state = setup_test_state().await;

        // Product exists but has no version (fresh install)
        let product = state
            .registry
            .get_product("wow")
            .expect("Product should exist");
        assert!(product.version.is_none());

        // Install any version should work
        let request = InstallRequest {
            build_id: Some(100),
            install_path: "/games/wow".to_string(),
            region: "us".to_string(),
            locale: "enUS".to_string(),
            tags: vec![],
            mode: InstallationMode::Casc,
        };

        let result = install_product(State(state), Path("wow".to_string()), Json(request)).await;

        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_product_code() {
        assert!(validate_product_code("wow").is_ok());
        assert!(validate_product_code("wow_classic").is_ok());
        assert!(validate_product_code("d3").is_ok());
        assert!(validate_product_code("hs").is_ok());

        assert!(validate_product_code("").is_err());
        assert!(validate_product_code("wow@test").is_err());
        assert!(validate_product_code("wow!").is_err());
        assert!(validate_product_code("wow test").is_err());
    }

    #[test]
    fn test_is_valid_region() {
        assert!(is_valid_region("us"));
        assert!(is_valid_region("eu"));
        assert!(is_valid_region("kr"));
        assert!(is_valid_region("cn"));
        assert!(is_valid_region("tw"));

        assert!(!is_valid_region("invalid"));
        assert!(!is_valid_region("US"));
        assert!(!is_valid_region(""));
    }

    #[test]
    fn test_is_valid_locale() {
        assert!(is_valid_locale("enUS"));
        assert!(is_valid_locale("enGB"));
        assert!(is_valid_locale("deDE"));
        assert!(is_valid_locale("frFR"));
        assert!(is_valid_locale("zhCN"));

        assert!(!is_valid_locale("invalid"));
        assert!(!is_valid_locale("enus"));
        assert!(!is_valid_locale(""));
    }

    #[tokio::test]
    async fn test_list_products_empty() {
        let state = setup_test_state().await;

        // List products (should only have the test product)
        let result = list_products(State(state.clone()))
            .await
            .expect("List should succeed");

        assert_eq!(result.0.products.len(), 1);
        assert!(result.0.links.contains_key("self"));
    }

    #[tokio::test]
    async fn test_list_products_multiple() {
        let state = setup_test_state().await;

        // Add more test products
        use crate::models::Product;
        let wow_classic = Product::new(
            "wow_classic".to_string(),
            "World of Warcraft Classic".to_string(),
        );
        let d3 = Product::new("d3".to_string(), "Diablo III".to_string());

        state
            .registry
            .create(&wow_classic)
            .expect("Failed to create product");
        state
            .registry
            .create(&d3)
            .expect("Failed to create product");

        // List products
        let result = list_products(State(state.clone()))
            .await
            .expect("List should succeed");

        assert_eq!(result.0.products.len(), 3); // wow (from setup) + wow_classic + d3
        assert!(result.0.links.contains_key("self"));

        // Verify product codes are present
        let codes: Vec<_> = result.0.products.iter().map(|p| &p.product_code).collect();
        assert!(codes.contains(&&"wow".to_string()));
        assert!(codes.contains(&&"wow_classic".to_string()));
        assert!(codes.contains(&&"d3".to_string()));
    }
}
