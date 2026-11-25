//! CLI Error Types

use std::io;
use thiserror::Error;

/// Result type for CLI operations
pub type CliResult<T> = Result<T, CliError>;

/// Error types for CLI
#[derive(Error, Debug)]
pub enum CliError {
    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Connection error
    #[error("Connection error: {message}")]
    Connection { message: String },

    /// RPC error
    #[error("RPC error: {message}")]
    Rpc { message: String },

    /// Invalid response
    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// Command not found
    #[error("Command not found: {0}")]
    CommandNotFound(String),

    /// Tab not found
    #[error("Tab not found: index {0}")]
    TabNotFound(usize),

    /// Server not running
    #[error("Server not running at {0}")]
    ServerNotRunning(String),

    /// Timeout
    #[error("Operation timed out")]
    Timeout,

    /// Invalid input
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Other error
    #[error("{0}")]
    Other(String),
}

impl From<serde_json::Error> for CliError {
    fn from(e: serde_json::Error) -> Self {
        CliError::Serialization(e.to_string())
    }
}

impl From<serde_yaml::Error> for CliError {
    fn from(e: serde_yaml::Error) -> Self {
        CliError::Serialization(e.to_string())
    }
}

impl From<uuid::Error> for CliError {
    fn from(e: uuid::Error) -> Self {
        CliError::InvalidInput(e.to_string())
    }
}

impl From<crate::ssh::SshError> for CliError {
    fn from(e: crate::ssh::SshError) -> Self {
        CliError::Other(e.to_string())
    }
}

impl CliError {
    /// Create a connection error
    pub fn connection<S: Into<String>>(message: S) -> Self {
        Self::Connection {
            message: message.into(),
        }
    }

    /// Create an RPC error
    pub fn rpc<S: Into<String>>(message: S) -> Self {
        Self::Rpc {
            message: message.into(),
        }
    }

    /// Create a config error
    pub fn config<S: Into<String>>(message: S) -> Self {
        Self::Config(message.into())
    }
    /// A mensagem no idioma da interface (#32). Em inglês é o `Display`;
    /// em português traduz o começo de cada tipo de erro (o detalhe que vem
    /// do sistema ou do servidor fica como veio).
    pub fn localized(&self) -> String {
        use crate::ui::i18n::{language, Language};
        if language() == Language::English {
            return self.to_string();
        }
        match self {
            Self::Io(e) => format!("Erro de E/S: {e}"),
            Self::Serialization(m) => format!("Erro ao ler ou gravar os dados: {m}"),
            Self::Connection { message } => format!("Erro de conexão: {message}"),
            Self::Rpc { message } => format!("Erro na chamada ao Code Terminal: {message}"),
            Self::InvalidResponse(m) => format!("Resposta inválida: {m}"),
            Self::Config(m) => format!("Erro de configuração: {m}"),
            Self::CommandNotFound(m) => format!("Comando não encontrado: {m}"),
            Self::TabNotFound(index) => format!("Aba não encontrada: índice {index}"),
            Self::ServerNotRunning(address) => {
                format!("O Code Terminal não está rodando em {address}")
            }
            Self::Timeout => "A operação passou do tempo".to_string(),
            Self::InvalidInput(m) => format!("Entrada inválida: {m}"),
            Self::Other(m) => m.clone(),
        }
    }

    /// Prefixo da linha de erro ("Error:"/"Erro:").
    pub fn prefix() -> &'static str {
        match crate::ui::i18n::language() {
            crate::ui::i18n::Language::English => "Error:",
            crate::ui::i18n::Language::PortugueseBr => "Erro:",
        }
    }
}

/// Exit codes for CLI
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum ExitCode {
    Success = 0,
    GeneralError = 1,
    ConnectionError = 2,
    RpcError = 3,
    InvalidArgs = 4,
    ServerNotRunning = 5,
    Timeout = 6,
}

impl ExitCode {
    /// Get the numeric exit code
    pub fn code(self) -> i32 {
        self as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_codes() {
        assert_eq!(ExitCode::Success.code(), 0);
        assert_eq!(ExitCode::GeneralError.code(), 1);
        assert_eq!(ExitCode::ConnectionError.code(), 2);
        assert_eq!(ExitCode::RpcError.code(), 3);
        assert_eq!(ExitCode::InvalidArgs.code(), 4);
        assert_eq!(ExitCode::ServerNotRunning.code(), 5);
        assert_eq!(ExitCode::Timeout.code(), 6);
    }

    #[test]
    fn test_cli_error_from_io() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let cli_err: CliError = io_err.into();

        match cli_err {
            CliError::Io(_) => {}
            _ => panic!("Expected Io error"),
        }
    }

    #[test]
    fn test_cli_error_from_serde_json() {
        let json_err = serde_json::from_str::<i32>("not a number").unwrap_err();
        let cli_err: CliError = json_err.into();

        match cli_err {
            CliError::Serialization(_) => {}
            _ => panic!("Expected Serialization error"),
        }
    }

    /// #32: mensagens de erro em português.
    #[test]
    fn test_localized_messages() {
        use crate::ui::i18n::{set_language, Language};
        let err = CliError::ServerNotRunning("/run/x.sock".into());
        set_language(Language::English);
        assert_eq!(err.localized(), err.to_string());
        assert_eq!(CliError::prefix(), "Error:");
        set_language(Language::PortugueseBr);
        assert_eq!(
            err.localized(),
            "O Code Terminal não está rodando em /run/x.sock"
        );
        assert_eq!(CliError::prefix(), "Erro:");
        assert_eq!(
            CliError::TabNotFound(3).localized(),
            "Aba não encontrada: índice 3"
        );
        assert_eq!(CliError::Timeout.localized(), "A operação passou do tempo");
        assert_eq!(CliError::Other("x".into()).localized(), "x");
        set_language(Language::English);
    }

    #[test]
    fn test_cli_error_constructors() {
        let err = CliError::connection("test");
        assert!(matches!(err, CliError::Connection { .. }));

        let err = CliError::rpc("test");
        assert!(matches!(err, CliError::Rpc { .. }));

        let err = CliError::config("test");
        assert!(matches!(err, CliError::Config(_)));
    }
}
