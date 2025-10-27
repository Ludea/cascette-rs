//! Distributed tracing setup using OpenTelemetry
//!
//! Implements FR-034: Distributed tracing spans for operations, network, database
//!
//! ## Features
//!
//! - OTLP gRPC exporter for trace data
//! - Span creation helpers for common patterns
//! - W3C Trace Context propagation
//! - Integration with tracing-subscriber

use anyhow::{Context, Result};
use opentelemetry::KeyValue;
use opentelemetry::trace::{SpanKind, TracerProvider};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    Resource,
    trace::{Config, Sampler, Tracer},
};
use std::time::Duration;
use tracing::Span;

/// Initialize OpenTelemetry tracer with OTLP exporter
///
/// # Configuration
///
/// - Exports to OTLP endpoint (typically <http://localhost:4317> for Jaeger/Tempo)
/// - Uses gRPC protocol
/// - Always-on sampling for development (use adaptive sampling in production)
/// - Service name: cascette-agent
///
/// # Examples
///
/// ```no_run
/// use cascette_agent::observability::tracing;
///
/// let tracer = tracing::init_tracer("http://localhost:4317").unwrap();
/// ```
pub fn init_tracer(endpoint: &str) -> Result<Tracer> {
    // Create OTLP exporter configuration
    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(endpoint)
        .with_timeout(Duration::from_secs(10));

    // Create pipeline
    let tracer_provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(
            Config::default()
                .with_sampler(Sampler::AlwaysOn)
                .with_resource(Resource::new(vec![
                    KeyValue::new("service.name", "cascette-agent"),
                    KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
                ])),
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio)
        .context("Failed to install OTLP tracer")?;

    // Get tracer from provider
    let tracer = tracer_provider.tracer("cascette-agent");

    tracing::info!(
        endpoint = %endpoint,
        "Distributed tracing initialized with OTLP exporter"
    );

    Ok(tracer)
}

/// Span creation helpers for common operation patterns
pub struct SpanBuilder;

// Future use: FR-034 (distributed tracing integration)
#[allow(dead_code)]
impl SpanBuilder {
    /// Create a span for an HTTP request
    ///
    /// Captures:
    /// - HTTP method
    /// - Request path
    /// - Remote address
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cascette_agent::observability::tracing::SpanBuilder;
    ///
    /// let span = SpanBuilder::http_request("GET", "/agent/products");
    /// ```
    #[must_use]
    pub fn http_request(method: &str, path: &str) -> Span {
        tracing::info_span!(
            "http_request",
            http.method = %method,
            http.route = %path,
            http.status_code = tracing::field::Empty,
            otel.kind = ?SpanKind::Server,
            otel.status_code = tracing::field::Empty,
        )
    }

    /// Create a span for an operation (install, update, repair, etc.)
    ///
    /// Captures:
    /// - Operation type
    /// - Product code
    /// - Operation ID
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cascette_agent::observability::tracing::SpanBuilder;
    /// use uuid::Uuid;
    ///
    /// let operation_id = Uuid::new_v4();
    /// let span = SpanBuilder::operation("install", "wow_classic", operation_id);
    /// ```
    #[must_use]
    pub fn operation(operation_type: &str, product_code: &str, operation_id: uuid::Uuid) -> Span {
        tracing::info_span!(
            "operation",
            operation.type = %operation_type,
            operation.product = %product_code,
            operation.id = %operation_id,
            operation.state = tracing::field::Empty,
            operation.progress_percent = tracing::field::Empty,
            otel.kind = ?SpanKind::Internal,
        )
    }

    /// Create a span for a network operation
    ///
    /// Captures:
    /// - Network operation type (download, query, etc.)
    /// - Target URL
    /// - Bytes transferred
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cascette_agent::observability::tracing::SpanBuilder;
    ///
    /// let span = SpanBuilder::network("download", "https://cdn.arctium.tools/...");
    /// ```
    #[must_use]
    pub fn network(operation: &str, url: &str) -> Span {
        tracing::info_span!(
            "network",
            network.operation = %operation,
            network.url = %url,
            network.bytes = tracing::field::Empty,
            network.duration_ms = tracing::field::Empty,
            otel.kind = ?SpanKind::Client,
        )
    }

    /// Create a span for a database operation
    ///
    /// Captures:
    /// - SQL operation type
    /// - Table name
    /// - Rows affected
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cascette_agent::observability::tracing::SpanBuilder;
    ///
    /// let span = SpanBuilder::database("INSERT", "operations");
    /// ```
    #[must_use]
    pub fn database(operation: &str, table: &str) -> Span {
        tracing::info_span!(
            "database",
            db.operation = %operation,
            db.table = %table,
            db.rows_affected = tracing::field::Empty,
            db.duration_ms = tracing::field::Empty,
            otel.kind = ?SpanKind::Client,
        )
    }

    /// Create a span for a file system operation
    ///
    /// Captures:
    /// - File operation type (read, write, verify)
    /// - File path (sanitized)
    /// - Bytes processed
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cascette_agent::observability::tracing::SpanBuilder;
    ///
    /// let span = SpanBuilder::file_operation("verify", "Data/data.000");
    /// ```
    #[must_use]
    pub fn file_operation(operation: &str, path: &str) -> Span {
        tracing::info_span!(
            "file_operation",
            file.operation = %operation,
            file.path = %path,
            file.bytes = tracing::field::Empty,
            otel.kind = ?SpanKind::Internal,
        )
    }
}

// Future use: Span completion tracking in operation executors
#[allow(dead_code)]
/// Helper to record span completion with status
pub fn record_span_status(span: &Span, success: bool, error_message: Option<&str>) {
    if success {
        span.record("otel.status_code", "OK");
    } else {
        span.record("otel.status_code", "ERROR");
        if let Some(msg) = error_message {
            span.record("error.message", msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_creation() {
        // Just verify spans can be created without panicking
        let _http = SpanBuilder::http_request("GET", "/test");
        let _op = SpanBuilder::operation("test", "product", uuid::Uuid::new_v4());
        let _net = SpanBuilder::network("download", "https://example.com");
        let _db = SpanBuilder::database("SELECT", "test_table");
        let _file = SpanBuilder::file_operation("read", "/path/to/file");
    }
}
