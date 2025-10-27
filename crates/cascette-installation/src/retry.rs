//! Enhanced retry logic for network operations
//!
//! This module provides smart retry capabilities with exponential backoff
//! and error classification for determining retry behavior.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio::time::sleep;

use crate::error::InstallationError;

/// Configuration for retry behavior
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial delay before first retry
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Factor to multiply delay by after each retry
    pub backoff_factor: f32,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            backoff_factor: 2.0,
        }
    }
}

/// Classification of errors for retry logic
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// Transient error that should be retried immediately
    Transient,
    /// Network error that should be retried with backoff
    Network,
    /// Configuration or authentication error that should not be retried
    Fatal,
}

/// Classify an error to determine retry behavior
#[must_use]
pub fn classify_error(error: &InstallationError) -> ErrorClass {
    match error {
        // Network errors are retryable with backoff
        InstallationError::NetworkError(msg) => {
            // Check for specific non-retryable network errors
            if msg.contains("404")
                || msg.contains("Not Found")
                || msg.contains("401")
                || msg.contains("403")
                || msg.contains("Unauthorized")
            {
                ErrorClass::Fatal
            } else {
                // All other network errors are retryable
                ErrorClass::Network
            }
        }

        // I/O errors might be transient
        InstallationError::IoError(io_err) => {
            let msg = io_err.to_string();
            if msg.contains("No space left") || msg.contains("Permission denied") {
                ErrorClass::Fatal
            } else if msg.contains("temporarily unavailable") {
                ErrorClass::Transient
            } else {
                ErrorClass::Network
            }
        }

        // Most other errors are fatal (configuration, parsing, etc.)
        _ => ErrorClass::Fatal,
    }
}

/// Retry executor that handles the retry logic
pub struct RetryExecutor {
    config: RetryConfig,
}

impl RetryExecutor {
    /// Create a new retry executor with default configuration
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: RetryConfig::default(),
        }
    }

    /// Create a new retry executor with custom configuration
    #[allow(dead_code)] // Used in tests
    #[must_use]
    pub fn with_config(config: RetryConfig) -> Self {
        Self { config }
    }

    /// Execute an operation with retry logic
    #[allow(dead_code)] // Used in tests
    pub async fn execute<F, T>(
        &self,
        mut operation: F,
        operation_name: &str,
    ) -> Result<T, InstallationError>
    where
        F: FnMut() -> Pin<Box<dyn Future<Output = Result<T, InstallationError>> + Send>>,
    {
        let mut attempt = 0;
        let mut delay = self.config.initial_delay;

        loop {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    let error_class = classify_error(&e);

                    // Check if we should retry
                    if error_class == ErrorClass::Fatal {
                        eprintln!("  ✗ Fatal error in {operation_name}: {e}");
                        return Err(e);
                    }

                    attempt += 1;

                    if attempt > self.config.max_retries {
                        eprintln!(
                            "  ✗ Failed {} after {} attempts: {}",
                            operation_name, self.config.max_retries, e
                        );
                        return Err(e);
                    }

                    // Log the retry attempt
                    eprintln!(
                        "  → Retry {}/{} for {} ({}): {}",
                        attempt,
                        self.config.max_retries,
                        operation_name,
                        match error_class {
                            ErrorClass::Transient => "transient",
                            ErrorClass::Network => "network",
                            ErrorClass::Fatal => "fatal", // Shouldn't reach here
                        },
                        e
                    );

                    // Wait before retry based on error class
                    match error_class {
                        ErrorClass::Transient => {
                            // Very short delay for transient errors
                            sleep(Duration::from_millis(100)).await;
                        }
                        ErrorClass::Network => {
                            // Exponential backoff for network errors
                            sleep(delay).await;
                            delay = Duration::from_secs_f32(
                                (delay.as_secs_f32() * self.config.backoff_factor)
                                    .min(self.config.max_delay.as_secs_f32()),
                            );
                        }
                        ErrorClass::Fatal => unreachable!(),
                    }
                }
            }
        }
    }

    /// Execute an operation with retry logic (simplified for non-async closures that return futures)
    pub async fn execute_async<F, Fut, T>(
        &self,
        operation: F,
        operation_name: &str,
    ) -> Result<T, InstallationError>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<T, InstallationError>>,
    {
        let mut attempt = 0;
        let mut delay = self.config.initial_delay;

        loop {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    let error_class = classify_error(&e);

                    // Check if we should retry
                    if error_class == ErrorClass::Fatal {
                        eprintln!("  ✗ Fatal error in {operation_name}: {e}");
                        return Err(e);
                    }

                    attempt += 1;

                    if attempt > self.config.max_retries {
                        eprintln!(
                            "  ✗ Failed {} after {} attempts: {}",
                            operation_name, self.config.max_retries, e
                        );
                        return Err(e);
                    }

                    // Log the retry attempt
                    eprintln!(
                        "  → Retry {}/{} for {} ({}): {}",
                        attempt,
                        self.config.max_retries,
                        operation_name,
                        match error_class {
                            ErrorClass::Transient => "transient",
                            ErrorClass::Network => "network",
                            ErrorClass::Fatal => "fatal", // Shouldn't reach here
                        },
                        e
                    );

                    // Wait before retry based on error class
                    match error_class {
                        ErrorClass::Transient => {
                            // Very short delay for transient errors
                            sleep(Duration::from_millis(100)).await;
                        }
                        ErrorClass::Network => {
                            // Exponential backoff for network errors
                            sleep(delay).await;
                            delay = Duration::from_secs_f32(
                                (delay.as_secs_f32() * self.config.backoff_factor)
                                    .min(self.config.max_delay.as_secs_f32()),
                            );
                        }
                        ErrorClass::Fatal => unreachable!(),
                    }
                }
            }
        }
    }
}

impl Default for RetryExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_classification() {
        // Network errors
        let error = InstallationError::NetworkError("Connection timeout".to_string());
        assert_eq!(classify_error(&error), ErrorClass::Network);

        let error = InstallationError::NetworkError("404 Not Found".to_string());
        assert_eq!(classify_error(&error), ErrorClass::Fatal);

        let error = InstallationError::NetworkError("401 Unauthorized".to_string());
        assert_eq!(classify_error(&error), ErrorClass::Fatal);

        // I/O errors
        // Note: Can't easily test IoError classification without creating actual io::Error instances

        // Other errors
        let error = InstallationError::Other("Invalid configuration".to_string());
        assert_eq!(classify_error(&error), ErrorClass::Fatal);
    }

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_delay, Duration::from_millis(500));
        assert_eq!(config.max_delay, Duration::from_secs(30));
        assert!((config.backoff_factor - 2.0).abs() < f32::EPSILON);
    }

    #[tokio::test]
    async fn test_retry_executor_success() {
        let executor = RetryExecutor::new();
        let mut call_count = 0;

        let result = executor
            .execute(
                || {
                    call_count += 1;
                    Box::pin(async move { Ok::<_, InstallationError>(42) })
                },
                "test operation",
            )
            .await;

        assert!(result.is_ok());
        assert_eq!(result.expect("Should succeed"), 42);
        assert_eq!(call_count, 1);
    }

    #[tokio::test]
    async fn test_retry_executor_network_error_recovery() {
        let executor = RetryExecutor::with_config(RetryConfig {
            max_retries: 3,
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(100),
            backoff_factor: 2.0,
        });

        let mut call_count = 0;

        let result = executor
            .execute(
                || {
                    call_count += 1;
                    Box::pin(async move {
                        if call_count <= 2 {
                            Err(InstallationError::NetworkError(
                                "Connection reset".to_string(),
                            ))
                        } else {
                            Ok(42)
                        }
                    })
                },
                "test operation",
            )
            .await;

        assert!(result.is_ok());
        assert_eq!(result.expect("Should succeed"), 42);
        assert_eq!(call_count, 3);
    }

    #[tokio::test]
    async fn test_retry_executor_fatal_error() {
        let executor = RetryExecutor::new();
        let mut call_count = 0;

        let result = executor
            .execute(
                || {
                    call_count += 1;
                    Box::pin(async move {
                        Err::<i32, _>(InstallationError::NetworkError("404 Not Found".to_string()))
                    })
                },
                "test operation",
            )
            .await;

        assert!(result.is_err());
        assert_eq!(call_count, 1); // Should not retry for fatal errors
    }

    #[tokio::test]
    async fn test_retry_executor_max_retries() {
        let executor = RetryExecutor::with_config(RetryConfig {
            max_retries: 2,
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(100),
            backoff_factor: 2.0,
        });

        let mut call_count = 0;

        let result = executor
            .execute(
                || {
                    call_count += 1;
                    Box::pin(async move {
                        Err::<i32, _>(InstallationError::NetworkError(
                            "Connection timeout".to_string(),
                        ))
                    })
                },
                "test operation",
            )
            .await;

        assert!(result.is_err());
        assert_eq!(call_count, 3); // Initial attempt + 2 retries
    }
}
