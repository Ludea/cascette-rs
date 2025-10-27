//! HTTP middleware for observability
//!
//! Implements request tracing, metrics collection, and structured logging
//! for all HTTP endpoints.
//!
//! ## Features
//!
//! - Request tracing with tower-http
//! - Metrics collection per endpoint
//! - Structured logging for all requests
//! - Response time tracking
//! - Error logging

use axum::{
    body::Body,
    extract::MatchedPath,
    http::{Request, Response},
    middleware::Next,
};
use std::time::Instant;
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

/// Create a tracing layer for HTTP requests
///
/// Configures tower-http `TraceLayer` with:
/// - Span creation for each request
/// - Request logging at DEBUG level
/// - Response logging at INFO level with latency in milliseconds
///
/// # Examples
///
/// ```ignore
/// use axum::Router;
/// use cascette_agent::observability::middleware;
///
/// let app = Router::new()
///     .layer(middleware::tracing_layer());
/// ```
#[must_use]
pub fn tracing_layer() -> tower_http::trace::TraceLayer<
    tower_http::classify::SharedClassifier<tower_http::classify::ServerErrorsAsFailures>,
    DefaultMakeSpan,
    DefaultOnRequest,
    DefaultOnResponse,
> {
    TraceLayer::new_for_http()
        .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
        .on_request(DefaultOnRequest::new().level(Level::DEBUG))
        .on_response(
            DefaultOnResponse::new()
                .level(Level::INFO)
                .latency_unit(tower_http::LatencyUnit::Millis),
        )
}

/// Metrics collection middleware
///
/// Records HTTP request metrics including:
/// - Request count by endpoint
/// - Response time distribution
/// - Error count by status code
///
/// # Examples
///
/// ```ignore
/// use axum::{Router, middleware};
/// use cascette_agent::observability::middleware as obs_middleware;
///
/// let app = Router::new()
///     .layer(middleware::from_fn(obs_middleware::metrics_middleware));
/// ```
pub async fn metrics_middleware(req: Request<Body>, next: Next) -> Response<Body> {
    let start = Instant::now();
    let method = req.method().clone();
    let path = req
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| req.uri().path().to_string(), |p| p.as_str().to_string());

    // Process request
    let response = next.run(req).await;

    // Record metrics
    let duration = start.elapsed();
    let status = response.status();

    tracing::debug!(
        method = %method,
        path = %path,
        status = %status,
        duration_ms = %duration.as_millis(),
        "HTTP request completed"
    );

    // TODO: Record metrics via metrics module when integrated
    // metrics.record_http_request(method, path, status, duration);

    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::get};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_tracing_layer() {
        let _layer = tracing_layer();
        // Layer creation should not panic
    }

    #[tokio::test]
    async fn test_metrics_middleware() {
        let app = Router::new()
            .route("/test", get(|| async { "Hello" }))
            .layer(axum::middleware::from_fn(metrics_middleware));

        let request = Request::builder()
            .uri("/test")
            .body(Body::empty())
            .expect("Failed to build test request");

        let response = app.oneshot(request).await.expect("Task should complete");
        assert_eq!(response.status(), 200);
    }
}
