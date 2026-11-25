//! CLI Configuration

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::cli::error::{CliError, CliResult};

/// CLI configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfig {
    /// Default socket path or TCP address
    pub socket_path: Option<PathBuf>,
    /// Default output format
    pub format: Option<String>,
    /// Timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Retry attempts
    #[serde(default = "default_retries")]
    pub retry_attempts: u32,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            socket_path: default_socket_path(),
            format: Some("text".to_string()),
            timeout_secs: default_timeout(),
            retry_attempts: default_retries(),
        }
    }
}

impl CliConfig {
    /// Load configuration from file or use defaults
    pub fn load(path: Option<&Path>) -> CliResult<Self> {
        let config_path = path
            .map(|p| p.to_path_buf())
            .or_else(|| dirs::config_dir().map(|d| d.join("code-terminal").join("cli.toml")));

        if let Some(path) = config_path {
            if path.exists() {
                let content = std::fs::read_to_string(&path)?;
                let config: CliConfig = toml::from_str(&content)
                    .map_err(|e| CliError::config(format!("Failed to parse config: {}", e)))?;
                return Ok(config);
            }
        }

        Ok(Self::default())
    }

    /// Save configuration to file
    pub fn save(&self, path: Option<&Path>) -> CliResult<()> {
        let config_path = path
            .map(|p| p.to_path_buf())
            .or_else(|| dirs::config_dir().map(|d| d.join("code-terminal").join("cli.toml")));

        if let Some(path) = config_path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let content = toml::to_string_pretty(self)
                .map_err(|e| CliError::config(format!("Failed to serialize config: {}", e)))?;
            std::fs::write(&path, content)?;
        }

        Ok(())
    }
}

fn default_socket_path() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        dirs::runtime_dir()
            .or_else(|| Some(PathBuf::from("/tmp")))
            .map(|d| d.join("code-terminal.sock"))
    }
    #[cfg(windows)]
    {
        None // Use TCP on Windows by default
    }
}

fn default_timeout() -> u64 {
    30
}

fn default_retries() -> u32 {
    3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_config_default() {
        let config = CliConfig::default();
        assert_eq!(config.timeout_secs, 30);
        assert_eq!(config.retry_attempts, 3);
        assert_eq!(config.format, Some("text".to_string()));
    }

    #[test]
    fn test_cli_config_serialization() {
        let config = CliConfig {
            socket_path: Some(PathBuf::from("/tmp/test.sock")),
            format: Some("json".to_string()),
            timeout_secs: 60,
            retry_attempts: 5,
        };

        let toml = toml::to_string(&config).unwrap();
        assert!(toml.contains("/tmp/test.sock"));
        assert!(toml.contains("json"));

        let deserialized: CliConfig = toml::from_str(&toml).unwrap();
        assert_eq!(config.timeout_secs, deserialized.timeout_secs);
        assert_eq!(config.retry_attempts, deserialized.retry_attempts);
    }

    #[test]
    fn test_cli_config_load_nonexistent() {
        // Should return default config when file doesn't exist
        let result = CliConfig::load(Some(Path::new("/nonexistent/path.toml")));
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.timeout_secs, 30);
    }
}
