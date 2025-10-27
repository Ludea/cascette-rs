//! Structured logging setup using tracing-subscriber
//!
//! Implements FR-033: Structured logging at debug, info, warn, error levels
//!
//! ## Features
//!
//! - JSON formatted logs for production environments
//! - Configurable log levels (trace, debug, info, warn, error)
//! - Structured fields for operation context
//! - Integration with OpenTelemetry tracing

use anyhow::Result;
use tracing::Level;
use tracing_subscriber::{EnvFilter, Layer, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Initialize structured logging
///
/// # Configuration
///
/// Configures tracing-subscriber with:
/// - JSON formatting for structured logs
/// - Configurable log level
/// - Timestamp and span information
/// - Integration with OpenTelemetry layer
///
/// # Examples
///
/// ```no_run
/// use cascette_agent::observability::logging;
///
/// // Initialize with info level
/// logging::init("info").unwrap();
///
/// // Initialize with debug level
/// logging::init("debug").unwrap();
/// ```
pub fn init(level: &str) -> Result<()> {
    let log_level = parse_log_level(level)?;

    // Create environment filter
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level.as_str()));

    // Create JSON formatter layer
    let fmt_layer = fmt::layer()
        .json()
        .with_current_span(true)
        .with_span_list(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .with_filter(env_filter);

    // Initialize subscriber with just the formatting layer
    // OpenTelemetry layer will be added separately if tracing is enabled
    // Ignore error if already initialized (common in tests)
    if matches!(
        tracing_subscriber::registry().with(fmt_layer).try_init(),
        Ok(())
    ) {
        tracing::info!(
            log_level = %log_level,
            "Structured logging initialized"
        );
    } else {
        // Already initialized, this is OK (common in test scenarios)
    }

    Ok(())
}

/// Initialize structured logging with OpenTelemetry integration
///
/// This version adds the OpenTelemetry tracing layer for distributed tracing.
///
/// # Examples
///
/// ```no_run
/// use cascette_agent::observability::{logging, tracing};
///
/// let tracer = tracing::init_tracer("http://localhost:4317").unwrap();
/// logging::init_with_tracing("info", tracer).unwrap();
/// ```
pub fn init_with_tracing(level: &str, tracer: opentelemetry_sdk::trace::Tracer) -> Result<()> {
    let log_level = parse_log_level(level)?;

    // Create environment filter
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level.as_str()));

    // Create JSON formatter layer
    let fmt_layer = fmt::layer()
        .json()
        .with_current_span(true)
        .with_span_list(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .with_filter(env_filter.clone());

    // Create OpenTelemetry layer
    let otel_layer = tracing_opentelemetry::layer()
        .with_tracer(tracer)
        .with_filter(env_filter);

    // Initialize subscriber with both layers
    // Ignore error if already initialized (common in tests)
    if matches!(
        tracing_subscriber::registry()
            .with(fmt_layer)
            .with(otel_layer)
            .try_init(),
        Ok(())
    ) {
        tracing::info!(
            log_level = %log_level,
            tracing_enabled = true,
            "Structured logging with distributed tracing initialized"
        );
    } else {
        // Already initialized, this is OK (common in test scenarios)
    }

    Ok(())
}

/// Parse log level string to tracing Level
fn parse_log_level(level: &str) -> Result<Level> {
    match level.to_lowercase().as_str() {
        "trace" => Ok(Level::TRACE),
        "debug" => Ok(Level::DEBUG),
        "info" => Ok(Level::INFO),
        "warn" => Ok(Level::WARN),
        "error" => Ok(Level::ERROR),
        _ => anyhow::bail!("Invalid log level: {level}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_log_level() {
        assert_eq!(
            parse_log_level("trace").expect("Failed to parse log level"),
            Level::TRACE
        );
        assert_eq!(
            parse_log_level("debug").expect("Failed to parse log level"),
            Level::DEBUG
        );
        assert_eq!(
            parse_log_level("info").expect("Failed to parse log level"),
            Level::INFO
        );
        assert_eq!(
            parse_log_level("warn").expect("Failed to parse log level"),
            Level::WARN
        );
        assert_eq!(
            parse_log_level("error").expect("Failed to parse log level"),
            Level::ERROR
        );

        // Case insensitive
        assert_eq!(
            parse_log_level("INFO").expect("Failed to parse log level"),
            Level::INFO
        );
        assert_eq!(
            parse_log_level("Debug").expect("Failed to parse log level"),
            Level::DEBUG
        );

        // Invalid level
        assert!(parse_log_level("invalid").is_err());
    }
}
