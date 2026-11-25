//! Controle remoto via socket
//!
//! Permite controlar o terminal de forma programática através de sockets
//! Unix (Linux/macOS) ou TCP (cross-platform) usando JSON-RPC 2.0.
//!
//! # Exemplo de uso
//!
//! ```rust,no_run
//! use code_terminal::remote::{RemoteServer, RemoteConfig};
//!
//! async fn start_server() {
//!     let config = RemoteConfig::with_tcp_addr("127.0.0.1:9876".parse().unwrap());
//!     let server = RemoteServer::new(config).unwrap();
//!     
//!     // Iniciar servidor (bloqueante)
//!     server.run().await.unwrap();
//! }
//! ```

pub mod auth;
pub mod commands;
pub mod config;
pub mod connection;
pub mod handler;
pub mod protocol;
pub mod security;
pub mod server;

pub use auth::{
    extract_bearer_token, AuthConfig, AuthContext, AuthError, AuthToken, AuthVerifier, Credentials,
};
pub use commands::{register_remote_commands, RemoteState, TabInfo, TerminalInfo, TerminalSize};
pub use config::{ListenAddress, RemoteConfig, RemoteConfigError};
pub use connection::{ConnectionId, ConnectionInfo, ConnectionManager};
pub use handler::{HandlerResult, RpcHandler, RpcMiddleware, TerminalRpcHandler};
pub use protocol::{
    error_codes, ErrorObject, Id, Message, MessageParser, Notification, Request, RequestBuilder,
    Response, RpcError, JSONRPC_VERSION,
};
pub use security::{
    SecurityAuditor, SecurityConfig, SecurityError, SecurityEvent, SecurityEventType,
    SecurityLevel, SecurityMiddleware, SecurityReport,
};
pub use server::{RemoteServer, RemoteServerBuilder, ServerState};

use std::fmt;

/// Erros do servidor remoto
#[derive(Debug)]
pub enum RemoteError {
    /// Servidor desabilitado na configuração
    Disabled,

    /// Erro de configuração
    Config(String),

    /// Erro ao fazer bind no endereço
    Bind(String),

    /// Estado inválido para operação
    InvalidState(String),

    /// Erro de IO
    Io(std::io::Error),

    /// Erro durante shutdown
    Shutdown(String),

    /// Erro de autenticação
    Authentication(String),

    /// Conexão recusada (max connections)
    ConnectionRefused,

    /// Timeout
    Timeout,
}

impl fmt::Display for RemoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RemoteError::Disabled => {
                write!(f, "Remote server is disabled in configuration")
            }
            RemoteError::Config(msg) => {
                write!(f, "Configuration error: {}", msg)
            }
            RemoteError::Bind(msg) => {
                write!(f, "Failed to bind: {}", msg)
            }
            RemoteError::InvalidState(state) => {
                write!(f, "Invalid server state: {}", state)
            }
            RemoteError::Io(e) => {
                write!(f, "IO error: {}", e)
            }
            RemoteError::Shutdown(msg) => {
                write!(f, "Shutdown error: {}", msg)
            }
            RemoteError::Authentication(msg) => {
                write!(f, "Authentication error: {}", msg)
            }
            RemoteError::ConnectionRefused => {
                write!(f, "Connection refused: max connections reached")
            }
            RemoteError::Timeout => {
                write!(f, "Operation timed out")
            }
        }
    }
}

impl std::error::Error for RemoteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RemoteError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for RemoteError {
    fn from(e: std::io::Error) -> Self {
        RemoteError::Io(e)
    }
}

impl From<RemoteConfigError> for RemoteError {
    fn from(e: RemoteConfigError) -> Self {
        RemoteError::Config(e.to_string())
    }
}

/// Resultado de operações remotas
pub type RemoteResult<T> = Result<T, RemoteError>;

/// Informações sobre o servidor
#[derive(Debug, Clone)]
pub struct ServerInfo {
    /// Estado atual
    pub state: ServerState,
    /// Endereço de escuta
    pub listen_address: Option<ListenAddress>,
    /// Número de conexões ativas
    pub connection_count: usize,
    /// Versão do protocolo
    pub protocol_version: String,
}

impl ServerInfo {
    /// Criar info com valores padrão
    pub fn new() -> Self {
        Self {
            state: ServerState::Stopped,
            listen_address: None,
            connection_count: 0,
            protocol_version: "1.0.0".to_string(),
        }
    }
}

impl Default for ServerInfo {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_error_display() {
        let err = RemoteError::Disabled;
        assert!(err.to_string().contains("disabled"));

        let err = RemoteError::Config("test".to_string());
        assert!(err.to_string().contains("test"));

        let err = RemoteError::ConnectionRefused;
        assert!(err.to_string().contains("refused"));
    }

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::other("test");
        let err: RemoteError = io_err.into();
        assert!(matches!(err, RemoteError::Io(_)));
    }

    #[test]
    fn test_server_info_default() {
        let info = ServerInfo::default();
        assert_eq!(info.state, ServerState::Stopped);
        assert_eq!(info.connection_count, 0);
        assert_eq!(info.protocol_version, "1.0.0");
    }

    #[test]
    fn test_remote_result_type() {
        let ok_result: RemoteResult<i32> = Ok(42);
        assert_eq!(ok_result.ok(), Some(42));

        let err_result: RemoteResult<i32> = Err(RemoteError::Disabled);
        assert!(err_result.is_err());
    }
}
