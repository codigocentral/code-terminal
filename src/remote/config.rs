//! Configuração do servidor remoto
//!
//! Define opções de conexão: Unix sockets (Linux/macOS) ou TCP (todos)

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;

/// Configuração do servidor remoto
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteConfig {
    /// Habilitar servidor remoto
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Caminho do socket Unix (Linux/macOS)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socket_path: Option<PathBuf>,

    /// Endereço TCP (fallback para Windows/cross-platform)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp_addr: Option<SocketAddr>,

    /// Número máximo de conexões simultâneas
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,

    /// Token de autenticação (opcional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,

    /// Timeout para conexões ociosas (segundos)
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_secs: u64,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            socket_path: default_socket_path(),
            tcp_addr: None,
            max_connections: default_max_connections(),
            auth_token: None,
            idle_timeout_secs: default_idle_timeout(),
        }
    }
}

fn default_enabled() -> bool {
    false
}

fn default_max_connections() -> usize {
    10
}

fn default_idle_timeout() -> u64 {
    300 // 5 minutos
}

#[cfg(unix)]
fn default_socket_path() -> Option<PathBuf> {
    Some(PathBuf::from("/tmp/code-terminal.sock"))
}

#[cfg(not(unix))]
fn default_socket_path() -> Option<PathBuf> {
    None
}

impl RemoteConfig {
    /// Criar configuração para testes
    pub fn for_testing() -> Self {
        Self {
            enabled: true,
            socket_path: None,
            tcp_addr: Some("127.0.0.1:0".parse().unwrap()), // Porta aleatória
            max_connections: 5,
            auth_token: None,
            idle_timeout_secs: 60,
        }
    }

    /// Criar configuração com socket Unix específico
    #[cfg(unix)]
    pub fn with_socket_path<P: Into<PathBuf>>(path: P) -> Self {
        Self {
            enabled: true,
            socket_path: Some(path.into()),
            tcp_addr: None,
            max_connections: default_max_connections(),
            auth_token: None,
            idle_timeout_secs: default_idle_timeout(),
        }
    }

    /// Criar configuração com endereço TCP específico
    pub fn with_tcp_addr(addr: SocketAddr) -> Self {
        Self {
            enabled: true,
            socket_path: None,
            tcp_addr: Some(addr),
            max_connections: default_max_connections(),
            auth_token: None,
            idle_timeout_secs: default_idle_timeout(),
        }
    }

    /// Verificar se configuração é válida
    pub fn validate(&self) -> Result<(), RemoteConfigError> {
        if !self.enabled {
            return Ok(());
        }

        if self.socket_path.is_none() && self.tcp_addr.is_none() {
            return Err(RemoteConfigError::NoListenAddress);
        }

        if self.max_connections == 0 {
            return Err(RemoteConfigError::InvalidMaxConnections);
        }

        Ok(())
    }

    /// Retornar endereço de escuta (socket ou TCP)
    pub fn listen_address(&self) -> ListenAddress {
        if let Some(path) = &self.socket_path {
            ListenAddress::Unix(path.clone())
        } else if let Some(addr) = self.tcp_addr {
            ListenAddress::Tcp(addr)
        } else {
            ListenAddress::None
        }
    }
}

/// Endereço de escuta do servidor
#[derive(Debug, Clone, PartialEq)]
pub enum ListenAddress {
    Unix(PathBuf),
    Tcp(SocketAddr),
    None,
}

impl ListenAddress {
    /// Retornar descrição formatada
    pub fn display(&self) -> String {
        match self {
            ListenAddress::Unix(path) => format!("unix:{}", path.display()),
            ListenAddress::Tcp(addr) => format!("tcp://{}", addr),
            ListenAddress::None => "none".to_string(),
        }
    }
}

/// Erros de configuração
#[derive(Debug, Clone, PartialEq)]
pub enum RemoteConfigError {
    NoListenAddress,
    InvalidMaxConnections,
    SocketPathTooLong,
}

impl std::fmt::Display for RemoteConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoteConfigError::NoListenAddress => {
                write!(
                    f,
                    "Nenhum endereço de escuta configurado (socket_path ou tcp_addr)"
                )
            }
            RemoteConfigError::InvalidMaxConnections => {
                write!(f, "max_connections deve ser maior que zero")
            }
            RemoteConfigError::SocketPathTooLong => {
                write!(f, "Caminho do socket Unix muito longo")
            }
        }
    }
}

impl std::error::Error for RemoteConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RemoteConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.max_connections, 10);
        assert_eq!(config.idle_timeout_secs, 300);
    }

    #[test]
    fn test_config_validation_ok() {
        let config = RemoteConfig::for_testing();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_disabled() {
        let config = RemoteConfig {
            enabled: false,
            socket_path: None,
            tcp_addr: None,
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_no_address() {
        let config = RemoteConfig {
            enabled: true,
            socket_path: None,
            tcp_addr: None,
            ..Default::default()
        };
        assert_eq!(config.validate(), Err(RemoteConfigError::NoListenAddress));
    }

    #[test]
    fn test_config_validation_zero_connections() {
        let config = RemoteConfig {
            enabled: true,
            socket_path: Some(PathBuf::from("/tmp/test.sock")),
            tcp_addr: None,
            max_connections: 0,
            auth_token: None,
            idle_timeout_secs: 300,
        };
        assert_eq!(
            config.validate(),
            Err(RemoteConfigError::InvalidMaxConnections)
        );
    }

    #[test]
    fn test_listen_address_tcp() {
        let config = RemoteConfig::with_tcp_addr("127.0.0.1:9876".parse().unwrap());
        assert_eq!(
            config.listen_address(),
            ListenAddress::Tcp("127.0.0.1:9876".parse().unwrap())
        );
    }

    #[test]
    fn test_listen_address_display() {
        let addr = ListenAddress::Tcp("127.0.0.1:9876".parse().unwrap());
        assert_eq!(addr.display(), "tcp://127.0.0.1:9876");

        let addr = ListenAddress::Unix(PathBuf::from("/tmp/test.sock"));
        assert_eq!(addr.display(), "unix:/tmp/test.sock");
    }
}
