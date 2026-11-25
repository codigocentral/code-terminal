//! Browser Error - Definições de erro do navegador

use thiserror::Error;

/// Resultado do browser
pub type BrowserResult<T> = Result<T, BrowserError>;

/// Erros do browser
#[derive(Error, Debug, Clone)]
pub enum BrowserError {
    #[error("Browser instance not found: {0}")]
    InstanceNotFound(String),

    #[error("WebView not initialized")]
    WebViewNotInitialized,

    #[error("Failed to create WebView: {0}")]
    WebViewCreationFailed(String),

    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    #[error("Navigation failed: {0}")]
    NavigationFailed(String),

    #[error("Load failed: {0}")]
    LoadFailed(String),

    #[error("JavaScript execution failed: {0}")]
    JavaScriptFailed(String),

    #[error("Script execution failed: {0}")]
    ScriptExecutionFailed(String),

    #[error("Not implemented: {0}")]
    NotImplemented(String),

    #[error("Platform not supported: {0}")]
    PlatformNotSupported(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Integration error: {0}")]
    IntegrationError(String),

    #[error("Invalid state: {0}")]
    InvalidState(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("IO error: {0}")]
    Io(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<std::io::Error> for BrowserError {
    fn from(e: std::io::Error) -> Self {
        BrowserError::Io(e.to_string())
    }
}

impl From<url::ParseError> for BrowserError {
    fn from(e: url::ParseError) -> Self {
        BrowserError::InvalidUrl(e.to_string())
    }
}
