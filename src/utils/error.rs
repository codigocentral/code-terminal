use thiserror::Error;

#[derive(Error, Debug)]
pub enum TerminalError {
    #[error("PTY error: {0}")]
    PtyError(String),
    #[error("Render error: {0}")]
    RenderError(String),
    #[error("Config error: {0}")]
    ConfigError(String),
}
