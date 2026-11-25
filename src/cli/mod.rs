//! Code Terminal CLI Module
//!
//! Ferramenta de linha de comando para controle remoto do terminal.

use clap::{Subcommand, ValueEnum};

pub mod agent_hook;
pub mod client;
pub mod commands;
pub mod config;
pub mod error;
pub mod help_pt;
pub mod preview;
#[cfg(test)]
pub(crate) mod test_support;

// Re-export types used by commands
pub use client::RemoteClient;
pub use config::CliConfig;
pub use error::{CliError, CliResult};

/// Output format for CLI commands
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
    Yaml,
}

/// Split direction for panes
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

/// Config management actions
#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Show current configuration
    Show,
    /// Set a configuration value
    Set { key: String, value: String },
    /// Get a configuration value
    Get { key: String },
    /// Reset configuration to defaults
    Reset,
}
