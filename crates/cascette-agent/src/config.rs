//! Configuration module for cascette-agent
//!
//! Provides TOML-based configuration with secure defaults:
//! - Localhost-default binding (FR-036)
//! - Port fallback sequence (1120, 6881-6883)
//! - Platform-specific config paths
//!
//! ## Configuration File Locations
//!
//! - Linux: `~/.config/cascette/agent.toml`
//! - macOS: `~/Library/Application Support/Cascette/agent.toml`
//! - Windows: `%APPDATA%\\Cascette\\agent.toml`
//!
//! ## Example Configuration
//!
//! ```toml
//! # Network configuration
//! bind_address = "127.0.0.1"  # Localhost-only by default (FR-036)
//! port = 1120                  # Primary port, falls back to 6881-6883
//!
//! # Database configuration
//! database_path = "~/.local/share/cascette/agent.db"
//!
//! # Logging configuration
//! log_level = "info"  # trace, debug, info, warn, error
//!
//! # Operation configuration
//! max_concurrent_operations = 1
//! operation_retention_days = 90
//! ```

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Agent service configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentConfig {
    /// Network configuration
    #[serde(default)]
    pub network: NetworkConfig,

    /// Database configuration
    #[serde(default)]
    pub database: DatabaseConfig,

    /// Logging configuration
    #[serde(default)]
    pub logging: LoggingConfig,

    /// Operation configuration
    #[serde(default)]
    pub operations: OperationsConfig,
}

/// Network binding configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Bind address (default: 127.0.0.1 for security - FR-036)
    pub bind_address: String,

    /// Primary port (default: 1120, Battle.net Agent compatibility)
    pub port: u16,

    /// Fallback ports if primary port is unavailable
    pub fallback_ports: Vec<u16>,
}

/// Database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Path to `SQLite` database file
    pub path: PathBuf,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    pub level: String,
}

/// Operations configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationsConfig {
    /// Maximum concurrent operations
    pub max_concurrent: usize,

    /// Operation retention in days (FR-031)
    pub retention_days: u32,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            // FR-036: Bind to localhost by default for security
            bind_address: "127.0.0.1".to_string(),
            // Battle.net Agent compatibility
            port: 1120,
            // Fallback ports
            fallback_ports: vec![6881, 6882, 6883],
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: default_database_path(),
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
        }
    }
}

impl Default for OperationsConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 1,
            retention_days: 90, // FR-031
        }
    }
}

impl AgentConfig {
    // Future use: T080 (wire up configuration loading)
    #[allow(dead_code)]
    /// Load configuration from file, falling back to defaults
    ///
    /// # Configuration Resolution
    ///
    /// 1. Try to load from specified path (if provided)
    /// 2. Try to load from platform-specific default path
    /// 3. Fall back to default configuration
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cascette_agent::config::AgentConfig;
    ///
    /// // Load from default location
    /// let config = AgentConfig::load(None::<&str>).expect("Failed to load config");
    ///
    /// // Load from specific path
    /// let config = AgentConfig::load(Some("/etc/cascette/agent.toml")).unwrap();
    /// ```
    pub fn load(path: Option<impl AsRef<Path>>) -> Result<Self> {
        // Try specified path first
        if let Some(path) = path {
            return Self::load_from_file(path.as_ref());
        }

        // Try platform-specific default path
        let default_path = default_config_path();
        if default_path.exists() {
            return Self::load_from_file(&default_path);
        }

        // Fall back to defaults
        Ok(Self::default())
    }

    // Helper for load() - Future use: T080
    #[allow(dead_code)]
    /// Load configuration from a specific file
    fn load_from_file(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let config: Self = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        config.validate()?;

        Ok(config)
    }

    // Future use: T080 (configuration validation)
    #[allow(dead_code)]
    /// Validate configuration
    ///
    /// # Validation Rules
    ///
    /// - Bind address must be valid IPv4 or IPv6
    /// - Port must be in valid range (1-65535)
    /// - Log level must be valid (trace, debug, info, warn, error)
    /// - Retention days must be positive
    pub fn validate(&self) -> Result<()> {
        // Validate bind address is a valid IP address
        if self
            .network
            .bind_address
            .parse::<std::net::IpAddr>()
            .is_err()
        {
            bail!("Invalid bind_address: {}", self.network.bind_address);
        }

        // Validate port range
        if self.network.port == 0 {
            bail!("Port cannot be 0");
        }

        // Validate fallback ports
        for port in &self.network.fallback_ports {
            if *port == 0 {
                bail!("Fallback port cannot be 0");
            }
        }

        // Validate log level
        match self.logging.level.to_lowercase().as_str() {
            "trace" | "debug" | "info" | "warn" | "error" => {}
            _ => bail!("Invalid log_level: {}", self.logging.level),
        }

        // Validate retention days
        if self.operations.retention_days == 0 {
            bail!("retention_days must be positive");
        }

        Ok(())
    }

    // Future use: Configuration persistence
    #[allow(dead_code)]
    /// Save configuration to file
    ///
    /// Creates parent directories if they don't exist.
    pub fn save(&self, path: &Path) -> Result<()> {
        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory: {}", parent.display())
            })?;
        }

        let contents = toml::to_string_pretty(self).context("Failed to serialize configuration")?;

        fs::write(path, contents)
            .with_context(|| format!("Failed to write config file: {}", path.display()))?;

        Ok(())
    }

    // Future use: T081 (server startup with port binding)
    #[allow(dead_code)]
    /// Get all ports to try (primary + fallbacks)
    #[must_use]
    pub fn all_ports(&self) -> Vec<u16> {
        let mut ports = vec![self.network.port];
        ports.extend(&self.network.fallback_ports);
        ports
    }
}

// Future use: T080 (configuration loading)
#[allow(dead_code)]
/// Get platform-specific default configuration path
///
/// - Linux: `~/.config/cascette/agent.toml`
/// - macOS: `~/Library/Application Support/Cascette/agent.toml`
/// - Windows: `%APPDATA%\\Cascette\\agent.toml`
#[must_use]
pub fn default_config_path() -> PathBuf {
    if let Some(config_dir) = dirs::config_dir() {
        config_dir.join("cascette").join("agent.toml")
    } else {
        PathBuf::from("agent.toml")
    }
}

/// Get platform-specific default database path
///
/// - Linux: `~/.local/share/cascette/agent.db`
/// - macOS: `~/Library/Application Support/Cascette/agent.db`
/// - Windows: `%APPDATA%\\Cascette\\agent.db`
#[must_use]
pub fn default_database_path() -> PathBuf {
    if let Some(data_dir) = dirs::data_local_dir() {
        data_dir.join("cascette").join("agent.db")
    } else {
        PathBuf::from("agent.db")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_config() {
        let config = AgentConfig::default();

        // FR-036: Default bind address must be localhost
        assert_eq!(config.network.bind_address, "127.0.0.1");

        // Battle.net Agent compatibility
        assert_eq!(config.network.port, 1120);
        assert_eq!(config.network.fallback_ports, vec![6881, 6882, 6883]);

        // FR-031: Default retention
        assert_eq!(config.operations.retention_days, 90);

        // Validate defaults
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_load_from_toml() {
        let toml_content = r#"
[network]
bind_address = "0.0.0.0"
port = 8080
fallback_ports = [8081, 8082]

[database]
path = "/var/lib/cascette/agent.db"

[logging]
level = "debug"

[operations]
max_concurrent = 2
retention_days = 30
"#;

        let mut file = NamedTempFile::new().expect("Failed to create temp file");
        file.write_all(toml_content.as_bytes())
            .expect("Failed to write test config");
        file.flush().expect("Failed to flush file");

        let config = AgentConfig::load(Some(file.path())).expect("Failed to load test config");

        assert_eq!(config.network.bind_address, "0.0.0.0");
        assert_eq!(config.network.port, 8080);
        assert_eq!(config.network.fallback_ports, vec![8081, 8082]);
        assert_eq!(
            config.database.path,
            PathBuf::from("/var/lib/cascette/agent.db")
        );
        assert_eq!(config.logging.level, "debug");
        assert_eq!(config.operations.max_concurrent, 2);
        assert_eq!(config.operations.retention_days, 30);
    }

    #[test]
    fn test_validation_invalid_bind_address() {
        let mut config = AgentConfig::default();
        config.network.bind_address = "not-an-ip".to_string();

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validation_invalid_port() {
        let mut config = AgentConfig::default();
        config.network.port = 0;

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validation_invalid_log_level() {
        let mut config = AgentConfig::default();
        config.logging.level = "invalid".to_string();

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validation_zero_retention() {
        let mut config = AgentConfig::default();
        config.operations.retention_days = 0;

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_all_ports() {
        let config = AgentConfig::default();
        let ports = config.all_ports();

        assert_eq!(ports, vec![1120, 6881, 6882, 6883]);
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let config = AgentConfig::default();
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");

        config
            .save(temp_file.path())
            .expect("Failed to save test config");
        let loaded =
            AgentConfig::load(Some(temp_file.path())).expect("Failed to load saved config");

        assert_eq!(config.network.bind_address, loaded.network.bind_address);
        assert_eq!(config.network.port, loaded.network.port);
        assert_eq!(
            config.operations.retention_days,
            loaded.operations.retention_days
        );
    }
}
