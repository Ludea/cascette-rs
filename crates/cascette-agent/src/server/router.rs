//! Axum router configuration for agent service
//!
//! Sets up all HTTP routes and middleware for the agent API.
//! Follows REST best practices with proper HTTP methods and status codes.

use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;
use std::time::Duration;
use tower_http::{cors::CorsLayer, timeout::TimeoutLayer, trace::TraceLayer};

use super::handlers;
use crate::state::AppState;

/// Create the main router with all agent API routes
///
/// Routes:
/// - GET /health - Service health check
/// - GET /metrics - Prometheus metrics endpoint
/// - GET /agent/products - List all products (US2)
/// - POST /products/:code/install - Install product
/// - POST /agent/update/:code - Update installed product (US2)
/// - GET /products/:code - Get product details
/// - GET /operations/:id - Get operation details
/// - GET /operations - List operations with filtering
/// - POST /operations/:id/cancel - Cancel operation
/// - GET /operations/:id/progress - Get operation progress (legacy)
/// - GET /products/:code/progress - Get product progress (legacy)
///
/// Middleware:
/// - `TraceLayer`: Request/response tracing (FR-034)
/// - `CorsLayer`: CORS support for web clients
/// - `TimeoutLayer`: 30s request timeout
pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        // Monitoring endpoints
        .route("/health", get(handlers::health_check))
        .route("/metrics", get(handlers::metrics))
        // Product management
        .route("/agent/products", get(handlers::list_products))
        .route(
            "/products/{product_code}/install",
            post(handlers::install_product),
        )
        .route(
            "/agent/update/{product_code}",
            post(handlers::update_product),
        )
        .route("/products/{product_code}", get(handlers::get_product))
        // Operation management
        .route("/operations/{operation_id}", get(handlers::get_operation))
        .route("/operations", get(handlers::list_operations))
        .route(
            "/operations/{operation_id}/cancel",
            post(handlers::cancel_operation),
        )
        // Legacy progress endpoints (compatibility)
        .route(
            "/operations/{operation_id}/progress",
            get(handlers::get_operation_progress),
        )
        .route(
            "/products/{product_code}/progress",
            get(handlers::get_product_progress),
        )
        // Add shared state
        .with_state(state)
        // Add middleware layers (applied in reverse order)
        .layer(TimeoutLayer::new(Duration::from_secs(30)))
        .layer(CorsLayer::permissive()) // TODO: Restrict in production
        .layer(TraceLayer::new_for_http())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{InstallationMode, Operation, OperationType, Priority};
    use crate::observability::Metrics;
    use crate::server::models::InstallRequest;
    use crate::state::{Database, OperationQueue, ProductRegistry};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use std::sync::Mutex;
    use tower::ServiceExt;

    fn create_test_state() -> Arc<AppState> {
        let db = Arc::new(Mutex::new(
            Database::in_memory().expect("Failed to create test database"),
        ));
        let queue = Arc::new(OperationQueue::new(db.clone()));
        let registry = Arc::new(ProductRegistry::new(db));
        let metrics = Arc::new(Metrics::new());

        let state = Arc::new(AppState::new(queue, registry, metrics));

        // Create test product to satisfy foreign key constraints
        use crate::models::Product;
        let product = Product::new("wow".to_string(), "World of Warcraft".to_string());
        let _ = state.registry.create(&product);

        state
    }

    #[tokio::test]
    async fn test_health_route() {
        let state = create_test_state();
        let router = create_router(state);

        let request = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .expect("Operation should succeed");

        let response = router.oneshot(request).await.expect("Task should complete");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_install_product_route() {
        let state = create_test_state();
        let router = create_router(state);

        let install_req = InstallRequest {
            build_id: Some(56313),
            install_path: "/games/wow".to_string(),
            region: "us".to_string(),
            locale: "enUS".to_string(),
            tags: vec![],
            mode: InstallationMode::Casc,
        };

        let body = serde_json::to_string(&install_req).expect("Failed to serialize JSON");

        let request = Request::builder()
            .method("POST")
            .uri("/products/wow/install")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .expect("Operation should succeed");

        let response = router.oneshot(request).await.expect("Task should complete");

        assert_eq!(response.status(), StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn test_get_operation_route() {
        let state = create_test_state();

        // Create operation
        let operation = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        let operation_id = operation.operation_id;
        state
            .queue
            .create(&operation)
            .expect("Failed to create operation");

        let router = create_router(state);

        let request = Request::builder()
            .uri(format!("/operations/{}", operation_id))
            .body(Body::empty())
            .expect("Operation should succeed");

        let response = router.oneshot(request).await.expect("Task should complete");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_list_operations_route() {
        let state = create_test_state();
        let router = create_router(state);

        let request = Request::builder()
            .uri("/operations")
            .body(Body::empty())
            .expect("Operation should succeed");

        let response = router.oneshot(request).await.expect("Task should complete");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_cancel_operation_route() {
        let state = create_test_state();

        // Create operation
        let operation = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        let operation_id = operation.operation_id;
        state
            .queue
            .create(&operation)
            .expect("Failed to create operation");

        let router = create_router(state);

        let request = Request::builder()
            .method("POST")
            .uri(format!("/operations/{}/cancel", operation_id))
            .body(Body::empty())
            .expect("Operation should succeed");

        let response = router.oneshot(request).await.expect("Task should complete");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_operation_not_found() {
        let state = create_test_state();
        let router = create_router(state);

        let request = Request::builder()
            .uri("/operations/00000000-0000-0000-0000-000000000001")
            .body(Body::empty())
            .expect("Operation should succeed");

        let response = router.oneshot(request).await.expect("Task should complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_metrics_route() {
        let state = create_test_state();
        let router = create_router(state);

        let request = Request::builder()
            .uri("/metrics")
            .body(Body::empty())
            .expect("Operation should succeed");

        let response = router.oneshot(request).await.expect("Task should complete");

        assert_eq!(response.status(), StatusCode::OK);

        // Verify content type
        let content_type = response
            .headers()
            .get("content-type")
            .expect("Content-Type header should be present");
        assert_eq!(content_type, "text/plain; version=0.0.4");
    }

    #[tokio::test]
    async fn test_list_products_route() {
        let state = create_test_state();
        let router = create_router(state);

        let request = Request::builder()
            .uri("/agent/products")
            .body(Body::empty())
            .expect("Operation should succeed");

        let response = router.oneshot(request).await.expect("Task should complete");

        assert_eq!(response.status(), StatusCode::OK);
    }
}
