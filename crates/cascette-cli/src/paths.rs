use anyhow::{Context, Result};
use std::path::PathBuf;

/// Get the configuration directory for cascette
pub fn config_dir() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .context("Could not determine config directory")?
        .join("cascette");

    // Ensure directory exists
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create config directory: {:?}", dir))?;

    Ok(dir)
}

/// Get the cache directory for cascette
pub fn cache_dir() -> Result<PathBuf> {
    let dir = dirs::cache_dir()
        .context("Could not determine cache directory")?
        .join("cascette");

    // Ensure directory exists
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create cache directory: {:?}", dir))?;

    Ok(dir)
}

/// Get the data directory for cascette (for storing CASC data)
pub fn data_dir() -> Result<PathBuf> {
    let dir = dirs::data_dir()
        .context("Could not determine data directory")?
        .join("cascette");

    // Ensure directory exists
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create data directory: {:?}", dir))?;

    Ok(dir)
}

/// Get the path to the main configuration file
pub fn config_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_paths() {
        // These should all succeed on supported platforms
        assert!(config_dir().is_ok());
        assert!(cache_dir().is_ok());
        assert!(data_dir().is_ok());
        assert!(config_file().is_ok());
    }
}
